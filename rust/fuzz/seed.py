"""Seed corpus of the `run` target: the Python blocks of the mdtests, the schema of their
first `sql` block, once per operation, with the cursor in the middle."""

import re
import sys
from pathlib import Path

root, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
block = re.compile(r"^```(\w+)[^\n]*\n(.*?)^```", re.M | re.S)
for page in sorted(root.rglob("*.md")):
    blocks = block.findall(page.read_text())
    schema = next((code for lang, code in blocks if lang == "sql"), "")
    for i, code in enumerate(code for lang, code in blocks if lang == "py" or lang == "python"):
        for op in range(4):
            head = 0x80 | (i + op) % 4 << 2 | op
            data = bytes([head]) + f"{code}\0{schema}".encode()
            (out / f"{page.parent.name}-{page.stem}-{i}-{op}").write_bytes(data)
