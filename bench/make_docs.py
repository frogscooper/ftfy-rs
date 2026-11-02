"""
Write realistic documents for throughput benchmarks: mostly clean text in
several scripts, the way real corpora look, rather than the fuzz corpus
(which is three-quarters mojibake).

Each document is about the given number of megabytes of UTF-8, built from
short paragraphs with a little noise mixed in: curly quotes, the odd HTML
entity, CRLF line endings, and roughly one line in 200 of real mojibake.

Usage:  python bench/make_docs.py outdir [megabytes]
"""

import random
import sys
from pathlib import Path

PARAGRAPHS = {
    "english": [
        "The committee met on Tuesday to review the budget for next year.",
        "She said the results were “encouraging, but not conclusive”.",
        "Shipping is free on orders over $50 & returns are accepted for 30 days.",
        "It’s the third time this month that the train has been late.",
        "Download the report (PDF, 2.4 MB) from the project website.",
        "Temperatures will rise to 24 degrees by the end of the week.",
    ],
    "french": [
        "Le comité s’est réuni mardi pour examiner le budget de l’année prochaine.",
        "À la fin de la journée, les résultats étaient très encourageants.",
        "La livraison est gratuite dès 50 € d’achat ; les retours sont acceptés.",
        "Noël approche et les marchés sont déjà très fréquentés.",
    ],
    "russian": [
        "Комитет собрался во вторник, чтобы рассмотреть бюджет.",
        "Погода на неделе будет тёплой и солнечной.",
        "Доставка бесплатная при заказе от 3000 рублей.",
    ],
    "chinese": [
        "委员会周二开会审查了明年的预算。",
        "她说结果“令人鼓舞，但并不确定”。",
        "本周末气温将升至二十四度。",
    ],
    "tweets": [
        "just shipped it \U0001F680\U0001F680 thanks everyone!!",
        "can’t believe it’s already november \U0001F342☕",
        "new blog post → https://example.com/posts/42 ✨",
        "café vibes ☕️ \U0001F44D\U0001F3FD",
    ],
}

MOJIBAKE = [
    "The cafÃ© is open until 10.",
    "Itâ€™s a long way to the top.",
    "ÐšÐ¾Ð¼Ð¸Ñ‚ÐµÑ‚",
    "å§”å‘˜ä¼š",
]


def make(lang, size):
    rng = random.Random(lang)
    lines = []
    total = 0
    while total < size:
        r = rng.random()
        if r < 0.005:
            line = rng.choice(MOJIBAKE)
        elif r < 0.01:
            line = rng.choice(PARAGRAPHS[lang]).replace(" ", " &amp; ", 1)
        else:
            line = " ".join(rng.choice(PARAGRAPHS[lang]) for _ in range(rng.randint(1, 4)))
        line += "\r\n" if rng.random() < 0.02 else "\n"
        lines.append(line)
        total += len(line.encode("utf-8"))
    return "".join(lines).encode("utf-8")


def main():
    out = Path(sys.argv[1])
    megabytes = float(sys.argv[2]) if len(sys.argv) > 2 else 10
    out.mkdir(parents=True, exist_ok=True)
    for lang in PARAGRAPHS:
        data = make(lang, int(megabytes * 1e6))
        (out / f"{lang}.txt").write_bytes(data)
        print(f"{lang:10} {len(data) / 1e6:6.1f} MB")


if __name__ == "__main__":
    main()
