"""Play the screenplay in tmux (Neovim left, shells right) and record each chapter with asciinema.

    uv run python record.py --nvim nvim --asciinema asciinema --chapters merge,rolling

Each scene is a chapter. The chapters before the ones asked for are played fast and
not recorded, to bring the app to where the chapter starts.
Needs build/cues.json from voice.py: each line's length paces what follows it.
Writes build/chapters/<name>.cast and build/chapters/<name>.json.
"""

import argparse
import json
import os
import random
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from kit import Fix, Hold, Keys, Open, Run, Say, Scene, Write
from screenplay import SCENES

HERE = Path(__file__).parent
BUILD = HERE / "build"
SESSION = "duckdesk"
SIZE = (200, 50)


class Director:
    def __init__(self, app: Path, nvim: str, cps: float) -> None:
        self.app, self.nvim, self.cps = app, nvim, cps
        self.fast = False
        self.state = Path(tempfile.mkdtemp(prefix="duckdesk-"))
        self.socket = self.state / "nvim.sock"
        self.cues = {
            cue["id"]: cue["duration"] for cue in json.loads((BUILD / "cues.json").read_text())
        }
        self.timeline: list[dict] = []
        self.speaking_until = 0.0
        self.t0 = 0.0

    def tmux(self, *args: str) -> str:
        return subprocess.run(
            ["tmux", "-L", SESSION, *args], check=True, capture_output=True, text=True
        ).stdout

    def start(self) -> None:
        env = f"DUCK_STATE={self.state} DUCKDESK_DIR={self.app}"
        shell = f"env {env} DUCK_PANE={{}} bash --rcfile {HERE / 'bashrc'} -i"
        cols, rows = SIZE
        self.tmux(
            "-f",
            str(HERE / "tmux.conf"),
            "new-session",
            "-d",
            "-s",
            SESSION,
            "-x",
            str(cols),
            "-y",
            str(rows),
            "-c",
            str(self.app),
            f"env {env} {self.nvim} -u {HERE / 'init.lua'} --listen {self.socket}",
        )
        self.tmux(
            "split-window",
            "-h",
            "-l",
            "78",
            "-t",
            f"{SESSION}:0.0",
            "-c",
            str(self.app),
            shell.format("shell"),
        )
        self.tmux(
            "split-window",
            "-v",
            "-l",
            "14",
            "-t",
            f"{SESSION}:0.1",
            "-c",
            str(self.app),
            shell.format("server"),
        )
        for pane, title in enumerate(["editor", "shell", "server"]):
            self.tmux("select-pane", "-t", f"{SESSION}:0.{pane}", "-T", title)
        self.tmux("select-pane", "-t", f"{SESSION}:0.0")
        for pane in ("shell", "server"):
            self.wait_prompt(pane, None)
        while not self.socket.exists():
            time.sleep(0.1)

    def pane(self, name: str) -> str:
        return f"{SESSION}:0.{['editor', 'shell', 'server'].index(name)}"

    def now(self) -> float:
        return time.time() - self.t0

    def lua(self, code: str) -> str:
        script = self.state / "action.lua"
        script.write_text(code)
        return subprocess.run(
            [
                self.nvim,
                "--server",
                str(self.socket),
                "--remote-expr",
                f'execute("luafile {script}")',
            ],
            check=True,
            capture_output=True,
            text=True,
        ).stdout

    def pause(self, seconds: float) -> None:
        time.sleep(0.05 if self.fast else seconds)

    def type(self, target: str, text: str) -> None:
        for line_number, line in enumerate(text.split("\n")):
            if line_number:
                self.tmux("send-keys", "-t", target, "Enter")
                self.pause(2.5 / self.cps)
            if self.fast:
                if line:
                    escaped = line[:-1] + "\\;" if line.endswith(";") else line
                    self.tmux("send-keys", "-t", target, "-l", escaped)
                continue
            for char in line:
                self.tmux("send-keys", "-t", target, "-l", "\\;" if char == ";" else char)
                time.sleep(random.uniform(0.6, 1.4) / self.cps)

    def prompt_state(self, pane: str) -> str | None:
        path = self.state / pane
        return path.read_text() if path.exists() else None

    def wait_prompt(self, pane: str, before: str | None) -> int:
        while (state := self.prompt_state(pane)) in (None, before):
            time.sleep(0.1)
        return int(state.split()[0])

    def say(self, action: Say) -> None:
        if self.fast:
            return
        self.wait_voice()
        start = self.now()
        self.speaking_until = start + self.cues[action.id] + 0.4
        self.timeline.append(
            {"id": action.id, "start": round(start, 3), "duration": self.cues[action.id]}
        )

    def wait_voice(self, extra: float = 0.0) -> None:
        if self.fast:
            return
        delay = self.speaking_until + extra - self.now()
        if delay > 0:
            time.sleep(delay)

    def run(self, action: Run) -> None:
        target = self.pane(action.pane)
        if action.cmd == "^C":
            self.tmux("send-keys", "-t", target, "C-c")
            self.pause(1)
            return
        before = self.prompt_state(action.pane)
        self.type(target, action.cmd)
        self.pause(0.4)
        self.tmux("send-keys", "-t", target, "Enter")
        if not action.wait:
            return
        status = self.wait_prompt(action.pane, before)
        if action.ok is not None and (status == 0) != action.ok:
            raise SystemExit(
                f"{action.cmd!r} exited with {status}, expected {'success' if action.ok else 'failure'}"
            )
        self.lua("vim.cmd('silent! checktime')")
        self.pause(1.2)

    def open(self, path: str) -> None:
        self.lua(f"vim.cmd.edit({json.dumps(path)})")
        self.pause(0.8)

    def write(self, action: Write) -> None:
        path = json.dumps(action.path)
        lines = action.text.removesuffix("\n").split("\n")
        place = {
            "new": "vim.fn.mkdir(vim.fn.fnamemodify(p, ':h'), 'p'); vim.cmd.edit(p); "
            "vim.api.nvim_buf_set_lines(0, 0, -1, false, {}); row = 0",
            "append": "vim.cmd.edit(p); row = vim.api.nvim_buf_line_count(0)",
            "after": "vim.cmd.edit(p); vim.api.nvim_win_set_cursor(0, {1, 0}); row = vim.fn.search(a, 'c')",
            "before": "vim.cmd.edit(p); vim.api.nvim_win_set_cursor(0, {1, 0}); row = vim.fn.search(a, 'c') - 1",
        }[action.where]
        anchor = json.dumps("\\V" + action.anchor.replace("\\", "\\\\"))
        if not action.typed or self.fast:
            self.lua(
                f"local p, a = {path}, {anchor}; {place}\n"
                f"vim.api.nvim_buf_set_lines(0, row, row, false, vim.json.decode({json.dumps(json.dumps(lines))}))\n"
                "if row == 0 and vim.api.nvim_buf_get_lines(0, -2, -1, false)[1] == '' then "
                "vim.api.nvim_buf_set_lines(0, -2, -1, false, {}) end\n"
                "vim.cmd('silent write'); vim.api.nvim_win_set_cursor(0, {math.max(row, 1), 0})"
            )
            self.pause(1.5)
            return
        self.lua(
            f"local p, a = {path}, {anchor}; {place}\n"
            "vim.api.nvim_buf_set_lines(0, row, row, false, {''})\n"
            "vim.api.nvim_win_set_cursor(0, {row + 1, 0}); vim.cmd('normal! zz')\n"
            "if vim.api.nvim_buf_line_count(0) == 2 and row == 0 then "
            "vim.api.nvim_buf_set_lines(0, 1, 2, false, {}) end"
        )
        editor = self.pane("editor")
        self.tmux("send-keys", "-t", editor, "i")
        self.type(editor, "\n".join(lines))
        self.tmux("send-keys", "-t", editor, "Escape")
        self.pause(0.3)
        self.type(editor, ":w")
        self.tmux("send-keys", "-t", editor, "Enter")
        self.pause(1.5)

    def keys(self, action: Keys) -> None:
        editor = self.pane("editor")
        *chunks, last = action.keys.split("<CR>")
        for chunk in chunks:
            self.type(editor, chunk)
            self.tmux("send-keys", "-t", editor, "Enter")
            self.pause(0.8)
        self.type(editor, last)

    def fix(self, action: Fix) -> None:
        self.open(action.path)
        find = (
            "for _, d in ipairs(vim.diagnostic.get(0)) do "
            f"if d.code == '{action.rule}' then "
            "print(vim.json.encode({d.lnum, d.col, d.end_lnum, d.end_col})) return end end"
        )
        deadline = time.time() + 30
        while not (found := self.lua(find).strip()) and time.time() < deadline:
            time.sleep(0.5)
        while found:
            lnum, col, end_lnum, end_col = json.loads(found)
            self.lua(f"vim.api.nvim_win_set_cursor(0, {{{lnum + 1}, {col}}})")
            self.pause(2.5)
            self.lua(
                "vim.lsp.buf.code_action({apply = true, "
                f"filter = function(a) return a.title == 'Fix {action.rule}' end, "
                f"range = {{start = {{{lnum + 1}, {col}}}, ['end'] = {{{end_lnum + 1}, {end_col}}}}}}})"
            )
            time.sleep(3)
            found = self.lua(find).strip()
        self.type(self.pane("editor"), ":w")
        self.tmux("send-keys", "-t", self.pane("editor"), "Enter")
        self.pause(1.5)

    def play(self, scene: Scene) -> None:
        for action in scene.actions:
            match action:
                case Say():
                    self.say(action)
                case Run():
                    self.run(action)
                case Write():
                    self.write(action)
                case Keys():
                    self.keys(action)
                case Open():
                    self.open(action.path)
                case Fix():
                    self.fix(action)
                case Hold():
                    self.wait_voice(action.seconds)

    def record(self, scene: Scene, asciinema: str) -> None:
        cols, rows = SIZE
        chapters = BUILD / "chapters"
        chapters.mkdir(parents=True, exist_ok=True)
        recorder = subprocess.Popen(
            [
                asciinema,
                "rec",
                "--headless",
                "--overwrite",
                "-q",
                "-f",
                "asciicast-v2",
                "--window-size",
                f"{cols}x{rows}",
                "-c",
                f"env TERM=xterm-256color tmux -L {SESSION} -u attach -t {SESSION}",
                str(chapters / f"{scene.name}.cast"),
            ],
        )
        self.t0, self.speaking_until, self.timeline = time.time(), 0.0, []
        time.sleep(1)
        try:
            self.play(scene)
            self.wait_voice(1)
        finally:
            subprocess.run(
                ["tmux", "-L", SESSION, "detach-client", "-s", SESSION], capture_output=True
            )
            recorder.wait(timeout=30)
        (chapters / f"{scene.name}.json").write_text(
            json.dumps({"cues": self.timeline, "end": round(self.now(), 3)}, indent=2)
        )
        print(f"chapter {scene.name}: {self.now():.0f} s recorded")


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--app", default=os.environ.get("DUCKDESK_DIR", str(Path.home() / "duckdesk"))
    )
    parser.add_argument("--nvim", default="nvim")
    parser.add_argument("--asciinema", default="asciinema")
    parser.add_argument("--cps", type=float, default=11, help="typing speed, characters per second")
    parser.add_argument("--chapters", help="comma-separated chapters to record (default: all)")
    args = parser.parse_args()

    app = Path(args.app)
    if (app / "compose.yaml").exists():
        subprocess.run(
            "docker ps -aq --filter label=app=duckdesk | xargs -r docker rm -f; docker compose down -v -t 1",
            shell=True,
            cwd=app,
            capture_output=True,
        )
    shutil.rmtree(app, ignore_errors=True)
    shutil.rmtree(app.parent / f"{app.name}-origin.git", ignore_errors=True)
    app.mkdir(parents=True)
    director = Director(app, args.nvim, args.cps)
    subprocess.run(["tmux", "-L", SESSION, "kill-server"], capture_output=True)
    director.start()
    names = [scene.name for scene in SCENES]
    wanted = args.chapters.split(",") if args.chapters else names
    unknown = set(wanted) - set(names)
    if unknown:
        raise SystemExit(
            f"unknown chapters: {', '.join(sorted(unknown))}; chapters: {', '.join(names)}"
        )
    last = max(names.index(name) for name in wanted)
    try:
        for scene in SCENES[: last + 1]:
            if scene.name in wanted:
                director.fast = False
                director.record(scene, args.asciinema)
            else:
                director.fast = True
                director.play(scene)
                print(f"chapter {scene.name}: played fast")
    finally:
        subprocess.run(["tmux", "-L", SESSION, "kill-server"], capture_output=True)


if __name__ == "__main__":
    main()
