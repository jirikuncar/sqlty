"""Download what the docs used to load from CDNs into ``docs/assets/vendor/``, so the site
and its playground work offline: Monaco, and the Inter and JetBrains Mono fonts.

    python3 scripts/vendor_assets.py

Each package is an npm tarball, checked against its registry integrity. ``make playground``
runs it; the output is not committed.
"""

from __future__ import annotations

import base64
import hashlib
import io
import shutil
import tarfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VENDOR = ROOT / "docs" / "assets" / "vendor"

PACKAGES = {
    "monaco": (
        "https://registry.npmjs.org/monaco-editor/-/monaco-editor-0.57.0.tgz",
        "sha512-5BkI9KGoqrNvBGUe15/QlZq3OooZ8WLg1AxTpaqHRCP3HNpzPPZKE2EDz8M7c+VRmCeUw1Brp4cx/PWm3kI/5A==",
        "package/min/",
    ),
    "inter": (
        "https://registry.npmjs.org/@fontsource-variable/inter/-/inter-5.3.0.tgz",
        "sha512-OupL48va4JNofb97w6NYeF9S7W/kHNKM0Er8Dem5nqi4jeOLrVJDoE8tZEpnMJmtkvNbB1EIPPwHcdkF6b1oUA==",
        "package/",
    ),
    "jetbrains-mono": (
        "https://registry.npmjs.org/@fontsource-variable/jetbrains-mono/-/jetbrains-mono-5.3.0.tgz",
        "sha512-F32xpS2NsGYoQi2ADSkKTgpJj7ozajsGgDJ8woTnqjmIB+dxDIqImjl4pXZVEExu8UFZ2ndhmX18EBS/hdz3Lw==",
        "package/",
    ),
}

# Monaco's language services the playground never starts (it uses its python, sql, ini and
# json modes), and its translations: two thirds of the package.
UNUSED = (
    "vs/language/typescript/",
    "vs/language/css/",
    "vs/language/html/",
    "vs/assets/ts.worker-",
    "vs/assets/css.worker-",
    "vs/assets/html.worker-",
    "vs/nls/",
)


def keep(name: str, path: str) -> bool:
    if name == "monaco":
        return not path.startswith(UNUSED) and not path.endswith(".map")
    return path in ("LICENSE", "index.css", "wght-italic.css") or (
        path.startswith("files/") and "-wght-" in path
    )


def main() -> None:
    for name, (url, integrity, prefix) in PACKAGES.items():
        target = VENDOR / name
        stamp = target / ".integrity"
        if stamp.exists() and stamp.read_text() == integrity:
            continue
        data = urllib.request.urlopen(url, timeout=60).read()
        digest = "sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode()
        if digest != integrity:
            raise SystemExit(f"{url}: expected {integrity}, got {digest}")
        shutil.rmtree(target, ignore_errors=True)
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            for member in archive.getmembers():
                path = member.name.removeprefix(prefix)
                if (
                    not member.isfile()
                    or not member.name.startswith(prefix)
                    or not keep(name, path)
                ):
                    continue
                out = target / path
                out.parent.mkdir(parents=True, exist_ok=True)
                out.write_bytes(archive.extractfile(member).read())
        stamp.write_text(integrity)
        print(f"{name}: {sum(1 for _ in target.rglob('*'))} files from {url}")


if __name__ == "__main__":
    main()
