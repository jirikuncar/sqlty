# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
"""Find small instruct GGUF models published since the last check, for the monthly ask eval.

    uv run scripts/ask_eval/new_models.py --days 35 --add

Prints a JSON list of the models it found. --add also appends them to models.json, so that
`evals.py ask --model ID` can load them. Only permissively licensed models of at most
--max-billions parameters with a Q4_K_M (or Q4_0) file count; ones the playground or
models.json already has are skipped.
"""

from __future__ import annotations

import argparse
import datetime
import json
import re
import time
import urllib.parse
import urllib.request
from pathlib import Path

HERE = Path(__file__).parent
API = "https://huggingface.co/api/models"
AUTHORS = ["Qwen", "ggml-org", "google", "HuggingFaceTB", "ibm-granite", "microsoft", "unsloth"]
LICENSES = {"apache-2.0", "mit"}
SKIP = re.compile(
    r"(?i)\b(base|vl|vision|omni|audio|speech|tts|asr|realtime|voxtral|embed\w*|rerank\w*|guard|math|draft|mtp)\b"
)
SIZE = re.compile(r"(?i)[-_](\d+(?:\.\d+)?)b\b")
QUANTS = (re.compile(r"(?i)q4_k_m\.gguf$"), re.compile(r"(?i)q4_0\.gguf$"))


def get(url: str, tries: int = 3):
    request = urllib.request.Request(url, headers={"User-Agent": "sqlty-ask-eval"})
    for attempt in range(tries):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return json.load(response)
        except OSError:
            if attempt == tries - 1:
                raise
            time.sleep(2**attempt)


def known() -> set[str]:
    ask = (HERE.parents[1] / "docs/assets/playground/ask.js").read_text()
    models = json.loads((HERE / "models.json").read_text())
    repos = {*re.findall(r'repo: "([^"]+)"', ask), *(model["repo"] for model in models)}
    return repos | {*re.findall(r'id: "([^"]+)"', ask), *(model["id"] for model in models)}


def candidate(repo: dict, since: datetime.datetime, limit: float) -> dict | None:
    name = repo["id"].split("/")[1]
    size = SIZE.search(name)
    created = datetime.datetime.fromisoformat(repo["createdAt"].replace("Z", "+00:00"))
    licenses = {
        tag.removeprefix("license:") for tag in repo.get("tags", []) if tag.startswith("license:")
    }
    if (
        repo.get("pipeline_tag", "text-generation") not in {None, "text-generation"}
        or created < since
        or SKIP.search(name)
    ):
        return None
    if not size or float(size[1]) > limit:
        return None
    if not licenses & LICENSES or repo.get("gated"):
        return None
    files = {entry["path"]: entry["size"] for entry in get(f"{API}/{repo['id']}/tree/main")}
    for quant in QUANTS:
        found = sorted(path for path in files if quant.search(path) and "/" not in path)
        if found:
            break
    else:
        return None
    slug = re.sub(r"(?i)-(gguf|instruct|it)\b", "", name).lower()
    return {
        "id": slug,
        "label": name.removesuffix("-GGUF"),
        "repo": repo["id"],
        "file": found[0],
        "bytes": files[found[0]],
        "license": sorted(licenses & LICENSES)[0],
        "created": repo["createdAt"][:10],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--days", type=int, default=35, help="look this many days back")
    parser.add_argument("--max-billions", type=float, default=4)
    parser.add_argument("--max", type=int, default=4, help="at most this many models")
    parser.add_argument("--add", action="store_true", help="append them to models.json")
    args = parser.parse_args()
    since = datetime.datetime.now(datetime.UTC) - datetime.timedelta(days=args.days)
    seen = known()
    found: list[dict] = []
    for author in AUTHORS:
        query = urllib.parse.urlencode(
            {"author": author, "search": "GGUF", "sort": "createdAt", "direction": -1, "limit": 100}
        )
        for repo in get(f"{API}?{query}"):
            if repo["id"] in seen or len(found) == args.max:
                continue
            if (model := candidate(repo, since, args.max_billions)) and model["id"] not in seen:
                seen |= {repo["id"], model["id"]}
                found.append(model)
    if args.add and found:
        path = HERE / "models.json"
        path.write_text(json.dumps(json.loads(path.read_text()) + found, indent=2) + "\n")
    print(json.dumps(found, indent=2))


if __name__ == "__main__":
    main()
