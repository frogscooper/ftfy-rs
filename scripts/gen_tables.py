"""
Generate crates/ftfy-rs/src/tables.rs from the reference Python ftfy.

Everything that defines ftfy's behavior -- its regexes, codec tables, HTML
entity list, and width map -- is read out of the installed `ftfy` package
rather than transcribed by hand, so the Rust port can't drift from it.

Python regexes are converted by walking their parsed form (re._parser) and
emitting equivalent Rust `regex` syntax, with every character escaped as
\\x{..}. Python's Unicode-aware \\w, \\s and \\d are expanded to the exact
codepoint sets this Python's `re` uses, since Rust's definitions differ.

Usage:  python scripts/gen_tables.py
"""

import re
import sys
import unicodedata
from pathlib import Path
from re import _constants as C
from re import _parser as P

import ftfy
import ftfy.badness
import ftfy.bad_codecs  # noqa: F401  (registers the sloppy codecs)
import ftfy.chardata as chardata

OUT = Path(__file__).resolve().parent.parent / "crates" / "ftfy-rs" / "src" / "tables.rs"


def char_ranges(pred):
    ranges = []
    start = None
    for cp in range(sys.maxunicode + 1):
        ok = pred(chr(cp))
        if ok and start is None:
            start = cp
        elif not ok and start is not None:
            ranges.append((start, cp - 1))
            start = None
    if start is not None:
        ranges.append((start, sys.maxunicode))
    return ranges


def esc(cp):
    return "\\x{%X}" % cp


def ranges_to_class_body(ranges):
    return "".join(esc(a) if a == b else esc(a) + "-" + esc(b) for a, b in ranges)


WORD = re.compile(r"\w")
SPACE = re.compile(r"\s")
DIGIT = re.compile(r"\d")
CATEGORIES = {
    C.CATEGORY_WORD: (char_ranges(lambda c: WORD.match(c) is not None), False),
    C.CATEGORY_NOT_WORD: (char_ranges(lambda c: WORD.match(c) is not None), True),
    C.CATEGORY_SPACE: (char_ranges(lambda c: SPACE.match(c) is not None), False),
    C.CATEGORY_NOT_SPACE: (char_ranges(lambda c: SPACE.match(c) is not None), True),
    C.CATEGORY_DIGIT: (char_ranges(lambda c: DIGIT.match(c) is not None), False),
    C.CATEGORY_NOT_DIGIT: (char_ranges(lambda c: DIGIT.match(c) is not None), True),
}


def emit_category(cat, in_class):
    ranges, negated = CATEGORIES[cat]
    body = ranges_to_class_body(ranges)
    if in_class:
        if negated:
            raise ValueError("negated category inside a class is not supported")
        return body
    return "[" + ("^" if negated else "") + body + "]"


def emit(subpattern):
    return "".join(emit_node(op, av) for op, av in subpattern)


def emit_node(op, av):
    if op is C.LITERAL:
        return esc(av)
    if op is C.NOT_LITERAL:
        return "[^" + esc(av) + "]"
    if op is C.ANY:
        return "."
    if op is C.IN:
        negate = False
        parts = []
        for iop, iav in av:
            if iop is C.NEGATE:
                negate = True
            elif iop is C.LITERAL:
                parts.append(esc(iav))
            elif iop is C.RANGE:
                parts.append(esc(iav[0]) + "-" + esc(iav[1]))
            elif iop is C.CATEGORY:
                parts.append(emit_category(iav, True))
            else:
                raise ValueError(f"unsupported class item {iop}")
        return "[" + ("^" if negate else "") + "".join(parts) + "]"
    if op is C.BRANCH:
        return "(?:" + "|".join(emit(alt) for alt in av[1]) + ")"
    if op is C.SUBPATTERN:
        _group, add_flags, del_flags, sub = av
        if add_flags or del_flags:
            raise ValueError("inline flags are not supported")
        return "(?:" + emit(sub) + ")"
    if op in (C.MAX_REPEAT, C.MIN_REPEAT):
        lo, hi, item = av
        if hi is C.MAXREPEAT:
            quant = "*" if lo == 0 else "+" if lo == 1 else "{%d,}" % lo
        elif lo == hi:
            quant = "{%d}" % lo
        elif (lo, hi) == (0, 1):
            quant = "?"
        else:
            quant = "{%d,%d}" % (lo, hi)
        if op is C.MIN_REPEAT:
            quant += "?"
        return "(?:" + emit(item) + ")" + quant
    if op is C.AT:
        if av is C.AT_BEGINNING or av is C.AT_BEGINNING_STRING:
            return "\\A"
        raise ValueError(f"unsupported anchor {av}")
    if op is C.CATEGORY:
        return emit_category(av, False)
    raise ValueError(f"unsupported regex op {op}")


def convert(pattern):
    return emit(P.parse(pattern.pattern, pattern.flags))


def convert_with_lookbehind(pattern):
    """
    Split a pattern of the form (?<! X) BODY into (X, BODY), since Rust's
    regex crate has no lookaround. The caller checks X by hand.
    """
    tree = P.parse(pattern.pattern, pattern.flags)
    (op, av), rest = tree[0], tree[1:]
    assert op is C.ASSERT_NOT and av[0] == -1, "expected a leading negative lookbehind"
    return emit(av[1]), "".join(emit_node(o, a) for o, a in rest)


def literal_set(op, av):
    """The characters a single-character node matches, or None if it isn't
    one (a repeat, an anchor, a category, ...)."""
    if op == C.LITERAL:
        return {av}
    if op != C.IN:
        return None
    chars = set()
    negated = False
    for item_op, item_av in av:
        if item_op == C.NEGATE:
            negated = True
        elif item_op == C.LITERAL:
            chars.add(item_av)
        elif item_op == C.RANGE:
            chars.update(range(item_av[0], item_av[1] + 1))
        else:
            return None
    return set(range(sys.maxunicode + 1)) - chars if negated else chars


def trigger_weight(cp):
    """How costly it is to have this character in the trigger set: how often
    it turns up in ordinary, correct text."""
    if cp < 0x80:
        return 10**6
    if (
        0xC0 <= cp <= 0x17F  # accented Latin letters
        or 0x386 <= cp <= 0x3CE  # the Greek alphabet
        or 0x410 <= cp <= 0x44F or cp in (0x401, 0x451)  # the Russian alphabet
        or 0x2010 <= cp <= 0x2027  # dashes, quotes, bullets
    ):
        return 1000
    if cp in (0xA0, 0xA9, 0xAB, 0xAE, 0xB0, 0xB7, 0xBB, 0x20AC):
        return 100
    return 1


def badness_triggers(pattern):
    """Necessary conditions for a match of the badness regex: every match
    contains one of the returned single characters, or one of the returned
    pairs of adjacent characters. Text with none of them can't be bad, so
    the regex can be skipped.

    The regex is one alternation of sequences. For each sequence we take one
    thing every match of it must contain: a character from one of its
    character classes, or two adjacent characters from two consecutive
    classes. We choose whichever is least likely in correct text, and only
    use classes without ASCII, so ASCII text never needs checking.

    Returns (singles, pairs): a list of character sets, and a list of
    (first, second) pairs of character sets.
    """
    if pattern.flags & re.IGNORECASE:
        raise ValueError("case-insensitive badness regex is not supported")
    (op, av), = P.parse(pattern.pattern, pattern.flags)
    if op != C.BRANCH:
        raise ValueError("expected the badness regex to be an alternation")
    singles, pairs = [], []
    for sequence in av[1]:
        classes = [literal_set(o, a) for o, a in sequence]
        options = []
        for i, chars in enumerate(classes):
            if chars and min(chars) >= 0x80:
                options.append((sum(map(trigger_weight, chars)), chars))
            if i > 0 and chars and classes[i - 1] and min(chars | classes[i - 1]) >= 0x80:
                first = classes[i - 1]
                cost = sum(map(trigger_weight, first)) * sum(map(trigger_weight, chars)) / 1e6
                options.append((cost, (first, chars)))
        if not options:
            raise ValueError("badness alternative without a non-ASCII class")
        _cost, best = min(options, key=lambda option: option[0])
        if isinstance(best, tuple):
            pairs.append(best)
        else:
            singles.append(best)
    return singles, pairs


def badness_classes(singles, pairs):
    """Number the characters by which of the sets they're in, to make the
    tables for the badness prefilter. Returns the ranges of characters in
    each class (class 0 is everything else, and isn't listed), whether each
    class is a trigger by itself, and for each class, the bitmask of the
    classes that are a trigger when they follow it."""
    sets = singles + [s for pair in pairs for s in pair]
    signature_class = {(): 0}
    char_class = {}
    for cp in sorted(set().union(*sets)):
        signature = tuple(i for i, s in enumerate(sets) if cp in s)
        char_class[cp] = signature_class.setdefault(signature, len(signature_class))
    signatures = sorted(signature_class, key=signature_class.get)
    if len(signatures) > 64:
        raise ValueError("too many badness character classes")

    def member(set_index):
        return {k for k, sig in enumerate(signatures) if set_index in sig}

    single = [False] * len(signatures)
    for i in range(len(singles)):
        for k in member(i):
            single[k] = True
    follows = [0] * len(signatures)
    for j in range(len(pairs)):
        first, second = member(len(singles) + 2 * j), member(len(singles) + 2 * j + 1)
        for a in first:
            for b in second:
                follows[a] |= 1 << b
    ranges = []
    for cp, k in sorted(char_class.items()):
        if ranges and ranges[-1][1] == cp - 1 and ranges[-1][2] == k:
            ranges[-1][1] = cp
        else:
            ranges.append([cp, cp, k])
    return ranges, single, follows


def cp_ranges(cps):
    ranges = []
    for cp in cps:
        if ranges and ranges[-1][1] == cp - 1:
            ranges[-1][1] = cp
        else:
            ranges.append([cp, cp])
    return ranges


def second_in_composition():
    """Characters that can compose with the character before them (their
    NFC_Quick_Check is Maybe)."""
    seconds = set(range(0x1161, 0x1176)) | set(range(0x11A8, 0x11C3))  # Hangul
    for cp in range(sys.maxunicode + 1):
        parts = unicodedata.decomposition(chr(cp)).split()
        if len(parts) == 2 and not parts[0].startswith("<"):
            first, second = (chr(int(p, 16)) for p in parts)
            if unicodedata.normalize("NFC", first + second) == chr(cp):
                seconds.add(ord(second))
    return seconds


def not_inert(form, seconds):
    """Characters that normalization `form` might change, or that might
    change a neighbor. Everything else is "inert": assigned, combining class
    0, unchanged by itself, and not able to compose with what's before it.
    Text of only inert characters is already normalized.

    This holds in any later Unicode version too, by the normalization
    stability policy. Unassigned characters count as not inert, since a
    later version may assign them."""
    def active(cp):
        ch = chr(cp)
        return (
            unicodedata.category(ch) == "Cn"
            or unicodedata.combining(ch) != 0
            or unicodedata.normalize(form, ch) != ch
            or (form in ("NFC", "NFKC") and cp in seconds)
        )
    return [r for r in cp_ranges(cp for cp in range(0x80, sys.maxunicode + 1) if active(cp))]


def rust_str(s):
    out = []
    for ch in s:
        cp = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif 0x20 <= cp < 0x7F:
            out.append(ch)
        else:
            out.append("\\u{%x}" % cp)
    return '"' + "".join(out) + '"'


def raw_str(s):
    assert '"#' not in s
    return 'r#"' + s + '"#'


def decode_table(encoding):
    table = []
    for b in range(256):
        try:
            table.append(ord(bytes([b]).decode(encoding)))
        except UnicodeDecodeError:
            table.append(0xFFFFFFFF)
    return table


def main():
    lines = [
        "// @generated by scripts/gen_tables.py from Python ftfy %s on Python %s"
        % (ftfy.__version__, "%d.%d" % sys.version_info[:2]),
        "// (Unicode %s). Do not edit by hand." % unicodedata.unidata_version,
        "",
        "#![allow(clippy::all, dead_code)]",
        "",
    ]

    lines.append("pub const BADNESS_PATTERN: &str = %s;" % raw_str(convert(ftfy.badness.BADNESS_RE)))
    # Every match of BADNESS_PATTERN contains a character of a class marked
    # in BADNESS_SINGLE, or a character of class b right after one of class
    # a, where bit b of BADNESS_FOLLOWS[a] is set.
    ranges, single, follows = badness_classes(*badness_triggers(ftfy.badness.BADNESS_RE))
    lines.append("pub const BADNESS_CLASSES: [(u32, u32, u8); %d] = [%s];" % (
        len(ranges), ", ".join("(0x%X, 0x%X, %d)" % tuple(r) for r in ranges)))
    lines.append("pub const BADNESS_SINGLE: [bool; %d] = [%s];" % (
        len(single), ", ".join("true" if s else "false" for s in single)))
    lines.append("pub const BADNESS_FOLLOWS: [u64; %d] = [%s];" % (
        len(follows), ", ".join("0x%X" % f for f in follows)))
    # Text without any of these is already in NFC or NFKC.
    seconds = second_in_composition()
    for form in ("NFC", "NFKC"):
        ranges = not_inert(form, seconds)
        lines.append("pub const %s_NOT_INERT: [(u32, u32); %d] = [%s];" % (
            form, len(ranges), ", ".join("(0x%X, 0x%X)" % (a, b) for a, b in ranges)))
    lookbehind, body = convert_with_lookbehind(chardata.UTF8_DETECTOR_RE)
    lines.append("pub const UTF8_DETECTOR_LOOKBEHIND: &str = %s;" % raw_str(lookbehind))
    lines.append("pub const UTF8_DETECTOR_PATTERN: &str = %s;" % raw_str(body))
    lines.append("")

    # Single-byte decoding tables. 0xFFFFFFFF marks a byte the codec can't decode.
    charmaps = [*chardata.CHARMAP_ENCODINGS, "windows-1252"]
    lines.append("pub const NO_CHAR: u32 = 0xFFFFFFFF;")
    lines.append("pub static CHARMAPS: [(&str, [u32; 256]); %d] = [" % len(charmaps))
    for enc in charmaps:
        table = decode_table(enc)
        rows = ", ".join("0x%X" % cp for cp in table)
        lines.append("    (%s, [%s])," % (rust_str(enc), rows))
    lines.append("];")
    lines.append("")

    entities = sorted(chardata.HTML_ENTITIES.items())
    lines.append("pub static HTML_ENTITIES: [(&str, &str); %d] = [" % len(entities))
    for name, value in entities:
        lines.append("    (%s, %s)," % (rust_str(name), rust_str(value)))
    lines.append("];")
    lines.append("")

    width = sorted(chardata.WIDTH_MAP.items())
    lines.append("pub static WIDTH_MAP: [(u32, &str); %d] = [" % len(width))
    for cp, value in width:
        lines.append("    (0x%X, %s)," % (cp, rust_str(value)))
    lines.append("];")
    lines.append("")

    ligatures = sorted(chardata.LIGATURES.items())
    lines.append("pub static LIGATURES: [(u32, &str); %d] = [" % len(ligatures))
    for cp, value in ligatures:
        lines.append("    (0x%X, %s)," % (cp, rust_str(value)))
    lines.append("];")
    lines.append("")

    controls = sorted(chardata.CONTROL_CHARS)
    lines.append("pub static CONTROL_CHARS: [u32; %d] = [%s];" % (
        len(controls), ", ".join("0x%X" % cp for cp in controls)))

    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KiB)")


if __name__ == "__main__":
    main()
