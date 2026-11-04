"""``--output-format sarif``: diagnostics as a SARIF 2.1.0 log, for GitHub code scanning
and other SARIF consumers."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

from sqlty._lint import DOCS_URL, RULES

_LEVELS = {"error": "error", "warn": "warning", "ignore": "none"}


def sarif_log(rows: Sequence[Mapping[str, Any]]) -> dict[str, object]:
    """The rows of ``--output-format json`` as one SARIF run; ``direction`` and ``against``
    of a ``sqlty breaking`` finding go to its ``properties``.

    Example:
        >>> log = sarif_log([])
        >>> log["version"], log["runs"][0]["tool"]["driver"]["name"]
        ('2.1.0', 'sqlty')
    """
    from sqlty import __version__

    rules = [
        {
            "id": rule.name,
            "shortDescription": {"text": rule.description},
            "helpUri": f"{DOCS_URL}reference/rules/#{rule.name}",
            "defaultConfiguration": {"level": _LEVELS[rule.default_severity.value]},
        }
        for rule in RULES.values()
    ]
    driver = {"name": "sqlty", "version": __version__, "informationUri": DOCS_URL, "rules": rules}

    def region(start: Mapping[str, int], end: Mapping[str, int]) -> dict[str, int]:
        return {
            "startLine": start["line"],
            "startColumn": start["column"],
            "endLine": end["line"],
            "endColumn": end["column"],
        }

    results: list[dict[str, object]] = []
    for row in rows:
        path = Path(row["file"])
        artifact = {"uri": path.as_uri() if path.is_absolute() else path.as_posix()}
        location = {"artifactLocation": artifact, "region": region(row["start"], row["end"])}
        result: dict[str, object] = {
            "ruleId": row["rule"],
            "level": _LEVELS[row["severity"]],
            "message": {"text": row["message"]},
            "locations": [{"physicalLocation": location}],
        }
        if row["fix"]:
            replacements = [
                {
                    "deletedRegion": region(edit["start"], edit["end"]),
                    "insertedContent": {"text": edit["replacement"]},
                }
                for edit in row["fix"]
            ]
            change = {"artifactLocation": artifact, "replacements": replacements}
            result["fixes"] = [{"artifactChanges": [change]}]
        if "direction" in row:
            result["properties"] = {"direction": row["direction"], "against": row["against"]}
        results.append(result)
    run = {"tool": {"driver": driver}, "columnKind": "unicodeCodePoints", "results": results}
    return {
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [run],
    }
