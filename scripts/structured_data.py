"""Add schema.org ``FAQPage`` JSON-LD to the FAQ page of a ``make docs`` build.

    python3 scripts/structured_data.py site

Each ``h2`` of the page is a question, and the text up to the next one its answer, so the
markup always says what the page shows.
"""

from __future__ import annotations

import json
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

PAGES = ("faq",)


class Questions(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.canonical = ""
        self.questions: list[dict[str, str]] = []
        self.article = False
        self.heading = False
        self.permalink = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = dict(attrs)
        if tag == "link" and attr.get("rel") == "canonical":
            self.canonical = attr["href"] or ""
        elif tag == "article":
            self.article = True
        elif self.article and tag == "h2":
            self.heading = True
            self.questions.append({"id": attr.get("id") or "", "name": "", "text": ""})
        elif tag == "a" and attr.get("class") == "headerlink":
            self.permalink = True

    def handle_endtag(self, tag: str) -> None:
        if tag == "article":
            self.article = False
        elif tag == "h2":
            self.heading = False
        elif tag == "a":
            self.permalink = False

    def handle_data(self, data: str) -> None:
        if not self.article or self.permalink or not self.questions:
            return
        self.questions[-1]["name" if self.heading else "text"] += data


def faq(html: str) -> dict[str, object]:
    parser = Questions()
    parser.feed(html)
    return {
        "@context": "https://schema.org",
        "@type": "FAQPage",
        "url": parser.canonical,
        "mainEntity": [
            {
                "@type": "Question",
                "name": squash(q["name"]),
                "acceptedAnswer": {
                    "@type": "Answer",
                    "text": squash(q["text"]),
                    "url": f"{parser.canonical}#{q['id']}",
                },
            }
            for q in parser.questions
        ],
    }


def squash(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def main() -> None:
    site = Path(sys.argv[1])
    for name in PAGES:
        page = site / name / "index.html"
        html = page.read_text()
        if '"FAQPage"' in html:
            continue
        data = json.dumps(faq(html), ensure_ascii=False).replace("</", "<\\/")
        page.write_text(
            html.replace("</head>", f'<script type="application/ld+json">{data}</script></head>', 1)
        )
        print(f"{page}: FAQPage")


if __name__ == "__main__":
    main()
