"""The acceptance spec of plans/2026-10-27-query-variants.md is frozen: only its xfail markers may go."""

import hashlib
import re
from pathlib import Path

SPEC = Path(__file__).parent / "mdtest" / "check" / "query_variants.md"
DIGEST = "febd8cbfaa94e556c05669a8b7d08535eba12fbb0a27e982583861ebeb101424"


def _normalized(text: str) -> str:
    text = re.sub(r"^<!-- mdtest:xfail query variants -->\n", "", text, flags=re.MULTILINE)
    return re.sub(r"\n{3,}", "\n\n", text)


def test_spec_is_frozen() -> None:
    digest = hashlib.sha256(_normalized(SPEC.read_text()).encode()).hexdigest()
    assert digest == DIGEST, "query_variants.md changed beyond removing xfail markers"
