"""Where SQLTy shows up in search: Search Console and Bing Webmaster query stats, Brave
Search ranks, Google and DuckDuckGo autocomplete, and best-effort result pages of Google,
Bing, DuckDuckGo and Seznam in headless Chromium (a page behind a bot check is recorded as
``blocked``, never solved). Each source runs only when its secret is set.

    uvx --python 3.13 --with google-auth --with requests --with playwright python scripts/search/check.py

Environment: ``GSC_SERVICE_ACCOUNT`` (service account JSON, added as a user of the
property), ``GSC_SITE`` (default ``sc-domain:sqlty.kuncar.dev``), ``BING_WEBMASTER_KEY``,
``BING_SITE`` (default ``https://sqlty.kuncar.dev/``), ``BRAVE_API_KEY``, ``DAYS`` (28).
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import pathlib
import re
import time
import urllib.parse
import urllib.request
from collections.abc import Callable, Iterator
from typing import Any

Row = dict[str, Any]

SERP = {
    "google": "https://www.google.com/search?hl=en&gl=us&num=20&pws=0&q={}",
    "bing": "https://www.bing.com/search?setlang=en&cc=us&q={}",
    "duckduckgo": "https://html.duckduckgo.com/html/?kl=us-en&q={}",
    "seznam": "https://search.seznam.cz/?q={}",
}
SUGGEST = {
    "google": "https://suggestqueries.google.com/complete/search?client=firefox&hl=en&q={}",
    "duckduckgo": "https://duckduckgo.com/ac/?type=list&q={}",
}
BLOCKED = re.compile(r"unusual traffic|captcha|not a robot|verify you are human|/sorry/", re.I)
OWN = re.compile(r"(^|\.)(google|bing|duckduckgo|seznam|microsoft|gstatic)\.")


def get_json(url: str, headers: dict[str, str] | None = None) -> Any:
    request = urllib.request.Request(
        url, headers={"User-Agent": "sqlty-search-check", **(headers or {})}
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def ranks(urls: list[str], targets: list[str]) -> dict[str, int | None]:
    return {t: next((i for i, u in enumerate(urls, 1) if t in u), None) for t in targets}


def search_console(days: int) -> Iterator[Row]:
    from google.auth.transport.requests import AuthorizedSession
    from google.oauth2 import service_account

    info = json.loads(os.environ["GSC_SERVICE_ACCOUNT"])
    scopes = ["https://www.googleapis.com/auth/webmasters.readonly"]
    session = AuthorizedSession(
        service_account.Credentials.from_service_account_info(info, scopes=scopes)
    )
    site = urllib.parse.quote(os.environ.get("GSC_SITE", "sc-domain:sqlty.kuncar.dev"), safe="")
    end = datetime.date.today() - datetime.timedelta(days=2)
    body = {
        "startDate": str(end - datetime.timedelta(days=days)),
        "endDate": str(end),
        "dimensions": ["query", "page"],
        "rowLimit": 1000,
    }
    response = session.post(
        f"https://searchconsole.googleapis.com/webmasters/v3/sites/{site}/searchAnalytics/query",
        json=body,
    )
    response.raise_for_status()
    for row in response.json().get("rows", []):
        query, page = row["keys"]
        yield {
            "query": query,
            "page": page,
            **{k: row[k] for k in ("clicks", "impressions", "ctr", "position")},
        }


def bing_webmaster(days: int) -> Iterator[Row]:
    site = urllib.parse.quote(os.environ.get("BING_SITE", "https://sqlty.kuncar.dev/"), safe="")
    key = os.environ["BING_WEBMASTER_KEY"]
    data = get_json(
        f"https://ssl.bing.com/webmaster/api.svc/json/GetQueryStats?siteUrl={site}&apikey={key}"
    )
    since = time.time() - days * 86400
    for row in data["d"]:
        stamp = int(re.search(r"\d+", row["Date"])[0]) / 1000
        if stamp >= since:
            yield {
                "query": row["Query"],
                "date": datetime.date.fromtimestamp(stamp).isoformat(),
                "clicks": row["Clicks"],
                "impressions": row["Impressions"],
                "position": row["AvgImpressionPosition"],
            }


def brave(queries: list[str], targets: list[str]) -> Iterator[Row]:
    headers = {"X-Subscription-Token": os.environ["BRAVE_API_KEY"], "Accept": "application/json"}
    for query in queries:
        data = get_json(
            f"https://api.search.brave.com/res/v1/web/search?count=20&q={urllib.parse.quote(query)}",
            headers,
        )
        urls = [r["url"] for r in data.get("web", {}).get("results", [])]
        yield {"query": query, "ranks": ranks(urls, targets), "top": urls[:10]}
        time.sleep(1.1)


def suggest(queries: list[str]) -> Iterator[Row]:
    for query in queries:
        for engine, url in SUGGEST.items():
            data = get_json(url.format(urllib.parse.quote(query)))
            yield {"engine": engine, "query": query, "suggestions": data[1]}


def result_pages(queries: list[str], targets: list[str], shots: pathlib.Path) -> Iterator[Row]:
    from playwright.sync_api import sync_playwright

    def unwrap(href: str) -> str | None:
        url = urllib.parse.urlsplit(href)
        params = urllib.parse.parse_qs(url.query)
        inner = (
            params.get("uddg")
            or params.get("url")
            or (params.get("q") if url.path == "/url" else None)
            or [href]
        )[0]
        return None if OWN.search(urllib.parse.urlsplit(inner).netloc) else inner

    shots.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as p:
        page = p.chromium.launch().new_page(locale="en-US")
        for query in queries:
            for engine, url in SERP.items():
                page.goto(url.format(urllib.parse.quote(query)), wait_until="domcontentloaded")
                page.wait_for_timeout(3000)
                page.screenshot(path=shots / f"{engine}-{re.sub(r'\W+', '_', query)}.png")
                if BLOCKED.search(page.url + page.inner_text("body")):
                    yield {"engine": engine, "query": query, "blocked": True}
                    continue
                hrefs = page.eval_on_selector_all('a[href^="http"]', "as => as.map(a => a.href)")
                urls = list(dict.fromkeys(u for u in map(unwrap, hrefs) if u))
                yield {
                    "engine": engine,
                    "query": query,
                    "blocked": False,
                    "ranks": ranks(urls, targets),
                    "top": urls[:10],
                }


def summary(results: dict[str, list[Row] | str], targets: list[str]) -> str:
    lines = ["# Search check", ""]
    for source, rows in results.items():
        lines += [f"## {source}", ""]
        if isinstance(rows, str):
            lines += [rows, ""]
        elif any("ranks" in r or "blocked" in r for r in rows):
            lines += [
                "| Engine | Query | " + " | ".join(targets) + " |",
                "|---" * (len(targets) + 2) + "|",
            ]
            for r in rows:
                cells = (
                    ["blocked"] * len(targets)
                    if r.get("blocked")
                    else [str(r["ranks"][t] or "-") for t in targets]
                )
                lines.append(
                    f"| {r.get('engine', source)} | {r['query']} | " + " | ".join(cells) + " |"
                )
        elif rows and "suggestions" in rows[0]:
            lines += [
                f"- {r['engine']} `{r['query']}`: {', '.join(r['suggestions'][:8]) or '-'}"
                for r in rows
            ]
        else:
            lines += ["| Query | Clicks | Impressions | Position |", "|---|---|---|---|"]
            top = sorted(rows, key=lambda r: -r["impressions"])[:30]
            lines += [
                f"| {r['query']} | {r['clicks']} | {r['impressions']} | {r['position']:.1f} |"
                for r in top
            ]
        lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--queries", type=pathlib.Path, default=pathlib.Path(__file__).with_name("queries.json")
    )
    parser.add_argument("--out", type=pathlib.Path, default=pathlib.Path("search-check"))
    parser.add_argument("--summary", type=pathlib.Path)
    parser.add_argument("--no-pages", action="store_true", help="skip the scraped result pages")
    args = parser.parse_args()

    config = json.loads(args.queries.read_text())
    queries, targets = config["queries"], config["targets"]
    days = int(os.environ.get("DAYS", "28"))
    sources: dict[str, tuple[str | None, Callable[[], Iterator[Row]]]] = {
        "Search Console": ("GSC_SERVICE_ACCOUNT", lambda: search_console(days)),
        "Bing Webmaster": ("BING_WEBMASTER_KEY", lambda: bing_webmaster(days)),
        "Brave Search": ("BRAVE_API_KEY", lambda: brave(queries, targets)),
        "Autocomplete": (None, lambda: suggest(queries)),
        "Result pages": (None, lambda: result_pages(queries, targets, args.out / "shots")),
    }
    if args.no_pages:
        del sources["Result pages"]

    args.out.mkdir(parents=True, exist_ok=True)
    results: dict[str, list[Row] | str] = {}
    for name, (secret, run) in sources.items():
        if secret and not os.environ.get(secret):
            results[name] = f"Skipped: `{secret}` is not set."
            continue
        try:
            results[name] = list(run())
        except Exception as error:  # one failing source must not hide the others
            results[name] = f"Failed: {type(error).__name__}: {error}"
    stamp = datetime.datetime.now(datetime.UTC).isoformat(timespec="seconds")
    (args.out / "results.json").write_text(
        json.dumps({"stamp": stamp, "results": results}, indent=1)
    )
    text = summary(results, targets)
    if args.summary:
        args.summary.write_text(text)
    print(text)


if __name__ == "__main__":
    main()
