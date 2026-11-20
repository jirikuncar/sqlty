"""Add machine-readable metadata to a ``make docs`` build.

    python3 scripts/structured_data.py site

- JSON-LD: ``BreadcrumbList`` on every inner page, ``TechArticle`` on guide and recipe pages,
  ``FAQPage`` on the FAQ, whose ``h2``s are the questions and the text up to the next one
  their answers, so the markup always says what the page shows.
- ``sitemap.xml``: ``lastmod`` of each URL.
- ``llms.txt``: a note on each link, from the page's description.

A page was modified when the last commit changed its source file. A shallow clone doesn't
know that, so its build gets no dates.
"""

from __future__ import annotations

import html as markup
import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ARTICLES = ("guide/", "recipes/")
AUTHOR = {"@type": "Person", "name": "Jiri Kuncar", "url": "https://github.com/jirikuncar"}


@dataclass
class Page:
    canonical: str = ""
    description: str = ""
    name: str = ""
    source: str = ""
    title: str = ""
    headline: str = ""
    questions: list[dict[str, str]] = field(default_factory=list)


class Parser(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.page = Page()
        self.article = False
        self.head = False
        self.title = False
        self.heading = False
        self.permalink = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        attr = dict(attrs)
        href = attr.get("href") or ""
        if tag == "link" and attr.get("rel") == "canonical":
            self.page.canonical = href
        elif tag == "meta" and attr.get("name") == "description":
            self.page.description = attr.get("content") or ""
        elif tag == "meta" and attr.get("property") == "og:site_name":
            self.page.name = attr.get("content") or ""
        elif tag == "a" and "/edit/main/docs/" in href:
            self.page.source = "docs/" + href.split("/edit/main/docs/", 1)[1]
        elif tag == "a" and attr.get("class") == "headerlink":
            self.permalink = True
        elif tag == "title":
            self.head = True
        elif tag == "article":
            self.article = True
        elif self.article and tag == "h1":
            self.title = True
        elif self.article and tag == "h2":
            self.heading = True
            self.page.questions.append({"id": attr.get("id") or "", "name": "", "text": ""})

    def handle_endtag(self, tag: str) -> None:
        if tag == "title":
            self.head = False
        elif tag == "article":
            self.article = False
        elif tag == "h1":
            self.title = False
        elif tag == "h2":
            self.heading = False
        elif tag == "a":
            self.permalink = False

    def handle_data(self, data: str) -> None:
        if self.head:
            self.page.headline += data
        if not self.article or self.permalink:
            return
        if self.title:
            self.page.title += data
        elif self.page.questions:
            self.page.questions[-1]["name" if self.heading else "text"] += data


def parse(html: str) -> Page:
    parser = Parser()
    parser.feed(html)
    parser.page.title = squash(parser.page.title)
    return parser.page


def squash(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def git(*args: str) -> str:
    try:
        return subprocess.run(
            ["git", "-C", str(ROOT), *args], capture_output=True, text=True
        ).stdout
    except OSError:
        return ""


def modified() -> dict[str, str]:
    """The ISO 8601 commit time of the last change to each file under ``docs/``."""
    if git("rev-parse", "--is-shallow-repository").strip() != "false":
        print("shallow or no git history: pages get no dates", file=sys.stderr)
        return {}
    log = git("log", "--no-renames", "--format=%x00%cI", "--name-only", "--", "docs")
    dates: dict[str, str] = {}
    for commit in log.split("\0")[1:]:
        date, *names = commit.split("\n")
        for name in filter(None, names):
            dates.setdefault(name, date)
    return dates


def faq(page: Page) -> dict[str, object]:
    return {
        "@context": "https://schema.org",
        "@type": "FAQPage",
        "url": page.canonical,
        "mainEntity": [
            {
                "@type": "Question",
                "name": squash(q["name"]),
                "acceptedAnswer": {
                    "@type": "Answer",
                    "text": squash(q["text"]),
                    "url": f"{page.canonical}#{q['id']}",
                },
            }
            for q in page.questions
        ],
    }


def breadcrumbs(site: Path, url: str, page: Page, home: Page) -> dict[str, object]:
    parts = url.strip("/").split("/")
    trail = [(home.name, home.canonical)]
    for depth in range(1, len(parts)):
        parent = site / "/".join(parts[:depth]) / "index.html"
        if parent.is_file():
            ancestor = parse(parent.read_text())
            trail.append((ancestor.title, ancestor.canonical))
    trail.append((page.title, page.canonical))
    return {
        "@context": "https://schema.org",
        "@type": "BreadcrumbList",
        "itemListElement": [
            {"@type": "ListItem", "position": n, "name": name, "item": link}
            for n, (name, link) in enumerate(trail, 1)
        ],
    }


def article(page: Page, date: str | None, home: Page) -> dict[str, object]:
    data: dict[str, object] = {
        "@context": "https://schema.org",
        "@type": "TechArticle",
        "headline": page.headline.removesuffix(f" - {home.name}"),
        "description": page.description,
        "url": page.canonical,
        "image": home.canonical + "assets/social.png",
        "author": AUTHOR,
        "isPartOf": {"@type": "WebSite", "name": home.name, "url": home.canonical},
    }
    if date:
        data["dateModified"] = date
    return data


def embed(html: str, *blocks: dict[str, object]) -> str:
    scripts = "".join(
        '<script type="application/ld+json">{}</script>'.format(
            json.dumps(block, ensure_ascii=False).replace("</", "<\\/")
        )
        for block in blocks
    )
    return html.replace("</head>", f"{scripts}</head>", 1)


def json_ld(site: Path, dates: dict[str, str]) -> dict[str, Page]:
    home = parse((site / "index.html").read_text())
    pages = {"": home}
    for path in sorted(site.rglob("index.html")):
        url = path.parent.relative_to(site).as_posix() + "/"
        if url == "./":
            continue
        html = path.read_text()
        page = pages[url] = parse(html)
        if '"BreadcrumbList"' in html or not page.source:
            continue
        blocks = [breadcrumbs(site, url, page, home)]
        if url.startswith(ARTICLES):
            blocks.append(article(page, dates.get(page.source), home))
        if url == "faq/":
            blocks.append(faq(page))
        path.write_text(embed(html, *blocks))
    return pages


def sitemap(site: Path, pages: dict[str, Page], dates: dict[str, str]) -> None:
    path = site / "sitemap.xml"
    root = pages[""].canonical

    def lastmod(match: re.Match[str]) -> str:
        page = pages.get(markup.unescape(match[1]).removeprefix(root))
        date = page and dates.get(page.source)
        return f"{match[0]}\n        <lastmod>{date}</lastmod>" if date else match[0]

    if "<lastmod>" not in (xml := path.read_text()):
        path.write_text(re.sub(r"<loc>([^<]*)</loc>", lastmod, xml))


def llms(site: Path, pages: dict[str, Page]) -> None:
    path = site / "llms.txt"
    root = pages[""].canonical

    def note(match: re.Match[str]) -> str:
        url = match[2].removeprefix(root).removesuffix("index.md")
        page = pages.get(url)
        if not page or not page.description:
            return match[0]
        return f"{match[0]}: {page.description.removeprefix(match[1] + ': ')}"

    path.write_text(re.sub(r"^- \[([^\]]+)\]\(<([^>]+)>\)$", note, path.read_text(), flags=re.M))


def main() -> None:
    site = Path(sys.argv[1])
    dates = modified()
    pages = json_ld(site, dates)
    sitemap(site, pages, dates)
    llms(site, pages)


if __name__ == "__main__":
    main()
