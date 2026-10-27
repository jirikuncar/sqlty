"""The acceptance spec of PLAN-QUERY-VARIANTS.md is frozen: only its xfail markers may go."""

import hashlib
import re
from pathlib import Path

SPEC = Path(__file__).parent / "mdtest" / "check" / "query_variants.md"
DIGEST = "d8b5de7e86d2b92a8ea2beb05f874f82ef95e62b4753482369770e11d86dab5e"


def _normalized(text: str) -> str:
    text = re.sub(r"^<!-- mdtest:xfail query variants -->\n", "", text, flags=re.MULTILINE)
    return re.sub(r"\n{3,}", "\n\n", text)


def test_spec_is_frozen() -> None:
    digest = hashlib.sha256(_normalized(SPEC.read_text()).encode()).hexdigest()
    assert digest == DIGEST, "query_variants.md changed beyond removing xfail markers"
