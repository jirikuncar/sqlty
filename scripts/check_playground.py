"""Load the docs playground in Chromium and check that it works: the engine answers, the
tour gets its diagnostics, a share link restores a session, steps move and reset, and
the site works offline once its service worker has cached it.

    uvx --with playwright python scripts/check_playground.py https://jirikuncar.github.io/sqlty/playground/
    uvx --with playwright python scripts/check_playground.py --site site

``--site`` serves a ``make docs`` build under ``/sqlty/``, the prefix of GitHub Pages, so a
path that only works at the root fails here as on the deployed site. Needs
``playwright install chromium``.
"""

from __future__ import annotations

import argparse
import contextlib
import functools
import http.server
import tempfile
import threading
from collections.abc import Iterator
from pathlib import Path

from playwright.sync_api import Page, sync_playwright

SOURCE = "monaco.editor.getEditors().find((e) => e.getContainerDomNode().matches('[data-source]'))"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("url", nargs="?", help="The playground page")
    target.add_argument("--site", type=Path, help="A `make docs` build to serve under /sqlty/")
    args = parser.parse_args()
    with (
        served(args.site) if args.site else contextlib.nullcontext(args.url) as url,
        sync_playwright() as playwright,
    ):
        browser = playwright.chromium.launch()
        page = browser.new_page()
        errors: list[str] = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.on("console", lambda message: message.type == "error" and errors.append(message.text))
        check(page, url)
        assert not errors, errors
        browser.close()
    print(f"{url}: the playground works")


def check(page: Page, url: str) -> None:
    page.goto(url)
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    problems = page.inner_text("[data-problems]")
    assert "unknown-column" in problems, problems

    page.evaluate(f"{SOURCE}.setValue('q = \"--sql\\\\nSELECT nme FROM users\"\\n')")
    page.wait_for_function("location.hash.length > 1")
    shared = page.url
    page.goto("about:blank")
    page.goto(shared)
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    assert page.evaluate(f"{SOURCE}.getValue()") == 'q = "--sql\\nSELECT nme FROM users"\n'
    page.click("[data-fix]")
    fixed = page.evaluate(f"{SOURCE}.getValue()")
    assert "SELECT name FROM users" in fixed, fixed

    page.select_option("[data-example]", label="Tour in steps")
    page.wait_for_selector("[data-steps]:not([hidden])")
    first = page.evaluate(f"{SOURCE}.getValue()")
    page.keyboard.press("PageDown")
    page.wait_for_function("document.querySelector('[data-step-select]').value === '1'")
    page.click("[data-source] .view-lines")
    page.keyboard.press("PageDown")
    page.keyboard.type(".")
    assert page.input_value("[data-step-select]") == "1"
    assert page.url.split("#")[0].endswith("/playground/"), page.url
    page.evaluate(f"{SOURCE}.setValue('edited = 1\\n')")
    page.click("[data-step-reset]")
    assert page.evaluate(f"{SOURCE}.getValue()") not in (first, "edited = 1\n")
    page.wait_for_timeout(500)
    stepped = page.url
    page.goto("about:blank")
    page.goto(stepped)
    page.wait_for_selector("[data-steps]:not([hidden])", timeout=60_000)
    assert page.input_value("[data-step-select]") == "1"

    embedded(page, url)
    offline(page, url)


def embedded(page: Page, url: str) -> None:
    """``?embed`` keeps a shared session with only the editor, the console and Fix all."""
    page.goto(url)
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    page.evaluate(f"{SOURCE}.setValue('q = \"--sql\\\\nSELECT nme FROM users\"\\n')")
    page.wait_for_function("location.hash.length > 1")
    shared = page.url.replace("#", "?embed#", 1)
    page.goto("about:blank")
    page.goto(shared)
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    assert not page.is_visible(".md-header")
    assert not page.is_visible("[data-example]")
    assert page.is_visible("[data-fix]")
    opened = page.get_attribute("[data-open]", "href")
    assert "embed" not in opened and opened.split("#")[1] == shared.split("#")[1], opened


def offline(page: Page, url: str) -> None:
    """The service worker of ``scripts/offline.py`` keeps the docs and the playground offline."""
    page.goto(url)
    page.wait_for_function("navigator.serviceWorker.controller !== null", timeout=60_000)
    cached = "caches.match(new URL('../assets/playground/sqlty_wasm.wasm', location.href))"
    page.wait_for_function(f"{cached}.then(Boolean)", polling=500, timeout=60_000)
    page.context.set_offline(True)
    try:
        page.goto(url + "?steps=../assets/playground/steps.json")
        page.wait_for_selector("[data-steps]:not([hidden])", timeout=60_000)
        problems = page.inner_text("[data-problems]")
        assert "stale-rewrite" in problems, problems
        page.goto(url.removesuffix("playground/") + "reference/rules/")
        assert page.title().startswith("Rules"), page.title()
    finally:
        page.context.set_offline(False)


@contextlib.contextmanager
def served(site: Path) -> Iterator[str]:
    with tempfile.TemporaryDirectory() as root:
        (Path(root) / "sqlty").symlink_to(site.resolve(), target_is_directory=True)
        handler = functools.partial(Quiet, directory=root)
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            yield f"http://127.0.0.1:{server.server_port}/sqlty/playground/"
        finally:
            server.shutdown()


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        pass


if __name__ == "__main__":
    main()
