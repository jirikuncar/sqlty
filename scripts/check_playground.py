"""Load the docs playground in Chromium and check that it works: the engine answers, the
tour gets its diagnostics, a share link restores a session, steps move and reset, and
a docs example opens in it, the site works offline once its service worker has cached
it, and every other control does what it shows. Fails when a line or function of
``playground.js`` never ran (V8 block coverage).

    uvx --with playwright python scripts/check_playground.py https://sqlty.kuncar.dev/playground/
    uvx --with playwright python scripts/check_playground.py --site site

``--site`` serves a ``make docs`` build under ``/sqlty/``, so a path that only works at the
host root fails here, as it would under a version prefix such as ``/latest/``. Needs
``playwright install chromium``.
"""

from __future__ import annotations

import argparse
import contextlib
import functools
import http.server
import json
import tempfile
import threading
from collections import Counter
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any

from playwright.sync_api import Browser, CDPSession, Page, sync_playwright

SOURCE = "monaco.editor.getEditors().find((e) => e.getContainerDomNode().matches('[data-source]'))"
MODEL = "monaco.editor.getModels().find((m) => m.getLanguageId() === '{}')"


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
        coverage = Coverage()
        page = browser.new_page()
        coverage.watch(page)
        errors: list[str] = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.on("console", lambda message: message.type == "error" and errors.append(message.text))
        check(page, url)
        assert not errors, errors
        exercise(browser, url, coverage.watch)
        coverage.take()
        script = page.request.get(f"{url}../assets/playground/playground.js").text()
        browser.close()
    lines, functions = coverage.missed(script)
    assert not lines and not functions, (
        f"playground.js never ran lines {lines} and functions {functions}"
    )
    print(f"{url}: the playground works, and every line of playground.js ran")


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

    page.evaluate(f'{SOURCE}.setValue(\'q = """--sql\\nselect id,name from users"""\\n\')')
    page.click("[data-commands]")
    page.click("[data-format]")
    assert not page.is_visible("#sqlty-commands")
    formatted = page.evaluate(f"{SOURCE}.getValue()")
    assert formatted == 'q = """--sql\nselect\n    id,\n    name\nfrom users"""\n', formatted

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
    example(page, url)
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


def example(page: Page, url: str) -> None:
    """A docs example opens in the playground with its schema, rules and diagnostics."""
    page.goto(url.removesuffix("playground/") + "guide/name-diagnostics/")
    page.click(".sqlty-try >> nth=0")
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    problems = page.inner_text("[data-problems]")
    assert "unknown-column" in problems, problems

    # Back in the docs and to the playground again, without a full load in between.
    page.evaluate(
        "document.querySelector('.md-nav__link[href$=\"/guide/name-diagnostics/\"]').click()"
    )
    page.wait_for_selector(".sqlty-try")
    page.evaluate("document.querySelectorAll('.sqlty-try')[1].click()")
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    shared = "import(new URL('../assets/playground/share.js', location.href)).then((m) => m.decode(location.hash.slice(1)))"
    assert page.evaluate(f"{shared}.then((s) => s.source)") == page.evaluate(f"{SOURCE}.getValue()")


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


# Lets a test trap the engine once, as a panic in the WebAssembly would, and records
# `window.open` instead of opening tabs. `?refuse` makes storage and full screen refuse.
INIT = """
const instantiate = WebAssembly.instantiate;
WebAssembly.instantiate = async (...args) => {
  const { instance } = await instantiate(...args);
  const { run } = instance.exports;
  const trap = () => {
    if (!globalThis.sqltyTrap) return run();
    globalThis.sqltyTrap = false;
    throw new WebAssembly.RuntimeError("unreachable");
  };
  return { instance: { exports: { ...instance.exports, run: trap } } };
};
window.opened = [];
window.open = (...args) => void window.opened.push(args);
if (new URLSearchParams(location.search).has("refuse")) {
  const { getItem, setItem } = Storage.prototype;
  Storage.prototype.getItem = function (key) { return key === "sqlty-playground" ? "{" : getItem.call(this, key); };
  Storage.prototype.setItem = function (key, value) {
    if (key === "sqlty-playground") throw new DOMException("denied", "SecurityError");
    setItem.call(this, key, value);
  };
  Element.prototype.requestFullscreen = () => Promise.reject(new DOMException("denied", "NotAllowedError"));
}
"""

# wllama with a scripted model: each completion is the next of `window.replies`.
WLLAMA = """
export class Wllama {
  setCompat() {}
  async loadModelFromUrl(url, { progressCallback }) { progressCallback({ loaded: 1 << 20, total: 2 << 20 }); }
  async createChatCompletion() { return { choices: [{ message: { content: window.replies.shift() } }] }; }
  async exit() { window.exited = (window.exited ?? 0) + 1; }
}
"""


def exercise(browser: Browser, url: str, watch: Callable[[Page], None]) -> None:
    """Every control of the playground, with what each one shows."""
    context = browser.new_context(permissions=["clipboard-read", "clipboard-write"])
    context.add_init_script(INIT)
    context.route(
        "**/vendor/wllama/esm/index.min.js",
        lambda route: route.fulfill(body=WLLAMA, content_type="text/javascript"),
    )
    page = context.new_page()
    errors: list[str] = []
    page.on("pageerror", lambda error: errors.append(str(error)))
    watch(page)

    page.goto(url + "?steps=missing.json")
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    assert "Cannot load the steps of missing.json: 404." in page.inner_text("[data-link-status]")

    page.evaluate("document.body.dataset.mdColorScheme = 'slate'")
    page.wait_for_selector("[data-source] .monaco-editor.vs-dark")

    files_and_panels(page)
    editing(page)
    engine_errors(page)
    commands(page)
    presenting(page)
    stepping(page)
    opening(page)
    sharing(page)
    resizing(page)
    zoomed(page, url)
    asking(page, url)
    assert not errors, errors


def source(page: Page, text: str | None = None) -> str:
    if text is not None:
        page.evaluate(f"(text) => {SOURCE}.setValue(text)", text)
        page.wait_for_timeout(300)
    return page.evaluate(f"{SOURCE}.getValue()")


def files_and_panels(page: Page) -> None:
    for name, language in [("schema", "sql"), ("config", "ini"), ("python", "python")]:
        page.click(f'[data-file="{name}"]')
        assert page.evaluate(f"{SOURCE}.getModel().getLanguageId()") == language
    for name in ["types", "json", "problems"]:
        page.click(f'[data-tab="{name}"]')
        assert page.is_visible(f'[data-panel="{name}"]')
    page.click('[data-file="schema"]')
    page.click("[data-problems] li button >> nth=0")
    assert page.evaluate(f"{SOURCE}.getModel().getLanguageId()") == "python"


def at(page: Page, text: str, offset: int = 0) -> None:
    """Put the cursor `offset` characters into the first `text` of queries.py."""
    page.evaluate(
        f"""([text, offset]) => {{
            const editor = {SOURCE};
            const position = editor.getModel().getPositionAt(editor.getValue().indexOf(text) + offset);
            editor.setPosition(position);
            editor.focus();
        }}""",
        [text, offset],
    )


def editing(page: Page) -> None:
    at(page, "SELECT o.id, u.name FROM", len("SELECT o.id, u."))
    page.evaluate(f"{SOURCE}.trigger('test', 'editor.action.triggerSuggest', null)")
    page.wait_for_selector(".suggest-widget .monaco-list-row")
    assert "name" in page.inner_text(".suggest-widget")
    page.keyboard.press("Escape")

    at(page, "SELECT o.id, u.name FROM", len("SELECT o.id, u.na"))
    page.evaluate(f"{SOURCE}.trigger('test', 'editor.action.showHover', null)")
    page.wait_for_selector(".monaco-hover-content code")
    page.keyboard.press("Escape")

    at(page, "SELECT nme", len("SELECT "))
    page.evaluate(f"{SOURCE}.trigger('test', 'editor.action.quickFix', null)")
    page.wait_for_selector("text=Fix unknown-column")
    page.keyboard.press("Enter")
    page.wait_for_timeout(300)
    assert "SELECT name FROM users" in source(page)

    at(page, "# A typo")
    page.evaluate(f"{SOURCE}.getAction('sqlty.explainRule').run()")
    at(page, "stale = SQL", len("stale = SQL["))
    page.evaluate(f"{SOURCE}.getAction('sqlty.explainRule').run()")
    opened = page.evaluate("window.opened.map(([url]) => new URL(url).hash)")
    assert opened[0] == "" and opened[1].startswith("#"), opened

    at(page, "SELECT")
    page.keyboard.press("Control+Shift+P")
    page.wait_for_selector(".quick-input-widget input")
    page.keyboard.press("Escape")


def engine_errors(page: Page) -> None:
    config = MODEL.format("ini")
    page.evaluate(f"{config}.setValue('[')")
    page.wait_for_function("document.querySelector('[data-problems]').textContent.includes('TOML')")
    page.evaluate(f"{config}.setValue('')")
    page.evaluate("globalThis.sqltyTrap = true")
    source(page, "x = 1\n")
    assert "RuntimeError: unreachable" in page.inner_text("[data-problems]")
    source(page, "y = 1\n")
    assert page.inner_text("[data-problems]") == "No problems."


def commands(page: Page) -> None:
    source(page, 'q = "--sql\\nSELECT id FROM users"\n')
    page.click("[data-commands]")
    page.wait_for_function("document.activeElement.matches('[data-rewrite]')")
    page.keyboard.press("ArrowDown")
    page.keyboard.press("ArrowUp")
    page.keyboard.press("x")
    assert page.evaluate("document.activeElement.matches('[data-rewrite]')")
    page.click("[data-rewrite]")
    assert not page.is_visible("#sqlty-commands")
    assert source(page).startswith("from sqlty import SQL"), source(page)
    page.click("[data-commands]")
    page.evaluate("dispatchEvent(new Event('scroll'))")
    page.click("[data-strip]")
    assert "SQL[" not in source(page), source(page)
    page.click("[data-fix]")
    page.click("[data-fix]")
    assert 'q = SQL[tuple[int]]("--sql' in source(page), source(page)

    page.click("[data-copy-link]")
    page.wait_for_function(
        "document.querySelector('[data-link-status]').textContent === 'Link copied.'"
    )
    assert page.evaluate("navigator.clipboard.readText()") == page.url
    page.evaluate(f"{SOURCE}.getAction('sqlty.copyLink').run()")


def presenting(page: Page) -> None:
    # Full screen changes after the click and the key: a late change ends the next presentation.
    settles = (
        "void (window.settled = new Promise((done) => {"
        " for (const name of ['fullscreenchange', 'fullscreenerror'])"
        " document.addEventListener(name, done, { once: true });"
        " setTimeout(done, 2000); }))"
    )
    page.evaluate(settles)
    page.click("[data-present]")
    assert page.get_attribute("[data-present]", "aria-pressed") == "true"
    page.evaluate("window.settled")
    page.evaluate(settles)
    page.keyboard.press("Escape")
    assert page.get_attribute("[data-present]", "aria-pressed") == "false"
    page.evaluate("window.settled")
    page.focus("[data-zoom='1']")
    page.keyboard.press("Control+Shift+F")
    assert page.get_attribute("[data-present]", "aria-pressed") == "true"
    # Leaving full screen, as Escape does where the page cannot lock it, stops presenting;
    # some headless browsers refuse full screen and only send the event.
    page.evaluate(
        "document.fullscreenElement ? document.exitFullscreen()"
        " : document.dispatchEvent(new Event('fullscreenchange'))"
    )
    page.wait_for_function(
        "!document.querySelector('#sqlty-playground').classList.contains('sqlty-present')"
    )
    page.click("[data-present]")
    page.click("[data-present]")
    assert page.get_attribute("[data-present]", "aria-pressed") == "false"


def stepping(page: Page) -> None:
    page.select_option("[data-example]", label="Tour in steps")
    page.wait_for_selector("[data-steps]:not([hidden])")
    page.click('[data-tab="notes"]')
    page.click('[data-step="1"]')
    page.click('[data-step="-1"]')
    page.select_option("[data-step-select]", "5")
    assert page.evaluate(f"{SOURCE}.getModel().getLanguageId()") == "sql"
    page.focus('[data-step="-1"]')
    page.keyboard.press("PageUp")
    assert page.input_value("[data-step-select]") == "4"
    page.click('[data-tab="notes"]')
    page.select_option("[data-example]", label="Tour")
    assert page.is_hidden("[data-steps]") and page.is_visible("[data-problems]")


DECK = {"steps": [{"source": "a = 1\n", "file": "config"}, {"title": "Two", "panel": "json"}]}


def opening(page: Page) -> None:
    for body, expected in [
        (json.dumps(DECK), "a = 1\n"),
        (json.dumps({"source": "b = 1\n", "dialect": "sqlite"}), "b = 1\n"),
    ]:
        with page.expect_file_chooser() as chosen:
            page.select_option("[data-example]", label="Open a file...")
        chosen.value.set_files(
            {"name": "deck.json", "mimeType": "application/json", "buffer": body.encode()}
        )
        page.wait_for_function(f"{MODEL.format('python')}.getValue() === {json.dumps(expected)}")
    assert page.input_value("[data-dialect]") == "sqlite"
    with page.expect_file_chooser() as chosen:
        page.select_option("[data-example]", label="Open a file...")
    chosen.value.set_files({"name": "bad.json", "mimeType": "application/json", "buffer": b"{"})
    page.wait_for_function(
        "document.querySelector('[data-link-status]').textContent.includes('JSON')"
    )
    with page.expect_file_chooser() as chosen:
        page.select_option("[data-example]", label="Open a file...")
    chosen.value.element.dispatch_event("cancel")
    page.wait_for_function("document.querySelector('[data-example]').value === '0'")

    for name, body in [("notes.txt", "x"), ("deck.json", json.dumps(DECK)), ("bad.json", "{")]:
        page.evaluate(
            """([name, body]) => {
                const data = new DataTransfer();
                data.items.add(new File([body], name));
                const root = document.getElementById("sqlty-playground");
                root.dispatchEvent(new DragEvent("dragover", { dataTransfer: data, bubbles: true, cancelable: true }));
                root.dispatchEvent(new DragEvent("drop", { dataTransfer: data, bubbles: true, cancelable: true }));
            }""",
            [name, body],
        )
    page.wait_for_function(
        "document.querySelector('[data-link-status]').textContent.startsWith('bad.json: ')"
    )
    assert page.input_value("[data-example]") == "-1"


def sharing(page: Page) -> None:
    encode = "(session) => import(new URL('../assets/playground/share.js', location.href)).then((m) => m.encode(session))"
    page.evaluate("location.hash = 'not-a-session'")
    good = page.evaluate(
        encode, {"source": "c = 1\n", "steps": [{"source": "c = 1\n"}, {}], "step": 1}
    )
    page.evaluate("(hash) => (location.hash = hash)", good)
    page.wait_for_function(f"{MODEL.format('python')}.getValue() === 'c = 1\\n'")
    assert page.input_value("[data-step-select]") == "1"
    bad = page.evaluate(encode, {"steps": [42]})
    page.evaluate("(hash) => (location.hash = hash)", bad)
    page.wait_for_function(
        "document.querySelector('[data-link-status]').textContent === 'Step 1 is not an object.'"
    )

    page.select_option("[data-example]", label="Tour")
    source(page, "".join(f"name_{i} = {i**7}\n" for i in range(4000)))
    page.wait_for_function(
        "document.querySelector('[data-link-status]').textContent.startsWith('Too large')"
    )
    page.click("[data-copy-link]")
    page.wait_for_timeout(300)
    assert "#" not in page.url


def resizing(page: Page) -> None:
    box = page.locator("[data-splitter]").bounding_box()
    page.mouse.move(box["x"] + 50, box["y"] + 2)
    page.mouse.down()
    page.mouse.move(box["x"] + 50, box["y"] - 60)
    page.mouse.up()
    page.focus("[data-splitter]")
    for key in ["ArrowUp", "ArrowDown", "Home"]:
        page.keyboard.press(key)
    height = page.evaluate(
        "getComputedStyle(document.getElementById('sqlty-playground')).getPropertyValue('--sqlty-console')"
    )
    assert height.endswith("px"), height


def zoomed(page: Page, url: str) -> None:
    page.click("[data-zoom='1']")
    page.focus("[data-zoom='1']")
    page.keyboard.press("Control+Equal")
    page.keyboard.press("Control+Minus")
    page.click(".md-header")
    page.keyboard.press("Control+Equal")
    page.goto(url)
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    assert (
        page.evaluate(
            "document.getElementById('sqlty-playground').style.getPropertyValue('--sqlty-font')"
        )
        == "14px"
    )
    page.goto(url + "?refuse")
    page.wait_for_selector("[data-types] code", state="attached", timeout=60_000)
    page.click("[data-zoom='-1']")
    page.click("[data-present]")
    assert page.get_attribute("[data-present]", "aria-pressed") == "true"
    page.click("[data-present]")


def asking(page: Page, url: str) -> None:
    page.goto(url + "?ask")
    page.wait_for_selector("[data-ask-open]:not([hidden])", timeout=60_000)
    page.click("[data-ask-open]")
    page.wait_for_function("document.activeElement.matches('[data-ask-question]')")
    page.select_option("[data-ask-model]", index=1)
    status = "document.querySelector('[data-ask-status]').textContent"

    source(page, "from sqlty import SQL\nanswer = 1\n")
    page.click("[data-ask] button")
    page.wait_for_function(f"{status}.startsWith('Type a question')")

    page.evaluate("window.replies = ['SELECT nme FROM users', 'SELECT name FROM users']")
    page.fill("[data-ask-question]", "Who?")
    page.click("[data-ask] button")
    page.wait_for_function(f"{status}.endsWith('feedback.')")
    assert "answer_2 = SQL[" in source(page), source(page)

    source(page, "x = 1\n# Every user\n")
    page.click("[data-source] .view-lines")
    at(page, "# Every")
    page.click("[data-ask-open]")
    page.select_option("[data-ask-model]", index=0)
    page.fill("[data-ask-question]", "")
    page.evaluate(
        "window.replies = ['SELECT nope FROM users', 'SELECT nope FROM users', 'SELECT nope FROM users']"
    )
    page.click("[data-ask] button")
    page.wait_for_function(f"{status}.endsWith('problem(s) left.')")
    assert page.evaluate("window.exited") == 1
    assert source(page) == 'x = 1\n# Every user\nanswer = """--sql\nSELECT nope FROM users"""\n', (
        source(page)
    )


class Coverage:
    """V8 block coverage of the playground's script, over every page and page load it watches."""

    def __init__(self) -> None:
        self.sessions: list[CDPSession] = []
        self.taken: list[list[dict[str, Any]]] = []

    def watch(self, page: Page) -> None:
        session = page.context.new_cdp_session(page)
        session.send("Profiler.enable")
        session.send("Profiler.startPreciseCoverage", {"callCount": True, "detailed": True})
        self.sessions.append(session)
        # The counts of a script go with the last page that holds it.
        goto = page.goto
        page.goto = lambda *args, **kwargs: (self.take(), goto(*args, **kwargs))[1]  # type: ignore[method-assign]

    def take(self) -> None:
        for session in self.sessions:
            for script in session.send("Profiler.takePreciseCoverage")["result"]:
                if script["url"].endswith("/playground/playground.js"):
                    self.taken.append(script["functions"])

    def missed(self, source: str) -> tuple[list[int], list[str]]:
        """The lines whose first character no load ran, and the functions none called."""
        lines = {}
        offset = 0
        for number, line in enumerate(source.splitlines(keepends=True), 1):
            code = line.lstrip()
            if code and not code.startswith("//"):
                lines[number] = offset + len(line) - len(code)
            offset += len(line)
        ran: set[int] = set()
        calls: Counter[tuple[str, int]] = Counter()
        for functions in self.taken:
            ranges = [r for function in functions for r in function["ranges"]]
            ran.update(number for number, at in lines.items() if count(ranges, at))
            for function in functions:
                name = function["functionName"] or "(anonymous)"
                first = function["ranges"][0]
                calls[name, first["startOffset"]] += first["count"]
        never = [
            f"{name} at line {source.count(chr(10), 0, start) + 1}"
            for (name, start), times in sorted(calls.items(), key=lambda call: call[0][1])
            if not times
        ]
        return sorted(set(lines) - ran), never


def count(ranges: list[dict[str, int]], at: int) -> int:
    """How often the innermost range around offset `at` ran."""
    holding = [r for r in ranges if r["startOffset"] <= at < r["endOffset"]]
    return min(holding, key=lambda r: r["endOffset"] - r["startOffset"])["count"]


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
