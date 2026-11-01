"""Screenplay actions, the step scripts they read code from, and how the voice says things."""

import re
from dataclasses import dataclass, field
from pathlib import Path

STEPS = Path(__file__).parent.parent / "steps"
HEREDOC = re.compile(r"cat (>>?) (\S+) <<'(\w+)'\n(.*?)\n\3\n", re.DOTALL)


def code(step: str, path: str, nth: int = 0) -> str:
    """The `nth` heredoc a step script writes to `path`: code("02", "queries.py", 1)."""
    script = next(STEPS.glob(f"{step}-*.sh")).read_text()
    bodies = [m.group(4) + "\n" for m in HEREDOC.finditer(script) if m.group(2) == path]
    return bodies[nth]


@dataclass
class Say:
    text: str
    id: str = ""


@dataclass
class Run:
    cmd: str
    pane: str = "shell"
    ok: bool | None = True
    wait: bool = True


@dataclass
class Write:
    """Show `text` in the editor: typed key by key, or pasted at once."""

    path: str
    text: str
    typed: bool = True
    where: str = "new"
    anchor: str = ""


@dataclass
class Keys:
    """Keys for the editor, typed: Keys(":%s/str/int/<CR>")."""

    keys: str


@dataclass
class Fix:
    """Apply the quick fix of every `rule` diagnostic in `path`, one at a time."""

    path: str
    rule: str = "unknown-column"


@dataclass
class Open:
    path: str


@dataclass
class Hold:
    """Wait until the voice has finished, then `seconds` more."""

    seconds: float = 0.5


@dataclass
class Scene:
    name: str
    actions: list = field(default_factory=list)


SPOKEN = [
    (r"`", ""),
    (r"\bsqlty\b", "sequel-tee"),
    (r"\bty\b", "tee-why"),
    (r"\buv\b", "you-vee"),
    (r"\bprek\b", "preck"),
    (r"\bpsycopg\b", "psycho-pee-gee"),
    (r"\bFastAPI\b", "Fast A P I"),
    (r"\bAPI\b", "A P I"),
    (r"\bSQL\b", "sequel"),
    (r"--sql", "dash dash sequel"),
    (r"\bNOT NULL\b", "not null"),
    (r"\bNUMERIC\b", "numeric"),
    (r"\bINSERT\b", "insert"),
    (r"\bUPDATE\b", "update"),
    (r"\bRETURNING\b", "returning"),
    (r"\bALTER TABLE\b", "alter table"),
    (r"\bCI\b", "C I"),
    (r"\bBookParams\b", "Book Params"),
    (r"\bhourly_rate\b", "hourly rate"),
    (r"\bduck_id\b", "duck I D"),
    (r"\bservice_version\b", "service version"),
    (r"\bopen_bugs\b", "open bugs"),
    (r"/backlog\b", "backlog"),
    (r"\bint\b", "int"),
    (r"\bstr\b", "string"),
    (r"\b003\b", "zero zero three"),
    (r"\b004\b", "zero zero four"),
    (r"\bLogfire\b", "Log-fire"),
]


def spoken(text: str) -> str:
    """What the voice reads for a line of narration."""
    for pattern, replacement in SPOKEN:
        text = re.sub(pattern, replacement, text)
    return text
