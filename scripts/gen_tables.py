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
        % (ftfy.__version__, sys.version.split()[0]),
        "// (Unicode %s). Do not edit by hand." % unicodedata.unidata_version,
        "",
        "#![allow(clippy::all, dead_code)]",
        "",
    ]

    lines.append("pub const BADNESS_PATTERN: &str = %s;" % raw_str(convert(ftfy.badness.BADNESS_RE)))
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
