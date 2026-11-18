"""Package the native ``sqlty`` executables of a release for npm and Homebrew.

    python scripts/package_binaries.py npm 0.7.0 dist/bin dist/npm
    python scripts/package_binaries.py homebrew 0.7.0 dist/tarballs > sqlty.rb

``dist/bin/<target>/`` holds the executable built for each Rust target, and
``dist/tarballs/`` the ``npm pack`` tarballs of the platform packages: the formula
downloads them from the npm registry, since an immutable GitHub Release takes no assets
once it is published.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = "https://registry.npmjs.org"
TARGETS: dict[str, tuple[str, str]] = {
    "aarch64-apple-darwin": ("darwin", "arm64"),
    "x86_64-apple-darwin": ("darwin", "x64"),
    "aarch64-unknown-linux-musl": ("linux", "arm64"),
    "x86_64-unknown-linux-musl": ("linux", "x64"),
    "aarch64-pc-windows-msvc": ("win32", "arm64"),
    "x86_64-pc-windows-msvc": ("win32", "x64"),
}
"""Rust target: the ``process.platform`` and ``process.arch`` of Node.js it runs on."""

FORMULA = """\
class Sqlty < Formula
  desc "Type-safe raw SQL for Python: row and parameter types from your SQL and schema"
  homepage "https://sqlty.kuncar.dev/"
  version "{version}"
  license "MIT"

  on_macos do
    on_arm do
{aarch64-apple-darwin}
    end
    on_intel do
{x86_64-apple-darwin}
    end
  end

  on_linux do
    on_arm do
{aarch64-unknown-linux-musl}
    end
    on_intel do
{x86_64-unknown-linux-musl}
    end
  end

  def install
    bin.install "bin/sqlty"
  end

  test do
    assert_equal "sqlty #{{version}}\\n", shell_output("#{{bin}}/sqlty version")
  end
end
"""


def npm(version: str, binaries: Path, out: Path) -> list[Path]:
    """Write the ``sqlty`` package and one package per platform to ``out``."""
    main = json.loads((ROOT / "npm/sqlty/package.json").read_text())
    written = []
    for target, (platform, arch) in TARGETS.items():
        name = f"@sqlty/cli-{platform}-{arch}"
        executable = "sqlty.exe" if platform == "win32" else "sqlty"
        package = out / name
        (package / "bin").mkdir(parents=True)
        shutil.copy2(binaries / target / executable, package / "bin" / executable)
        (package / "bin" / executable).chmod(0o755)
        manifest = {
            "name": name,
            "version": version,
            "description": f"The sqlty executable for {platform} {arch}",
            "homepage": main["homepage"],
            "repository": main["repository"],
            "license": main["license"],
            "os": [platform],
            "cpu": [arch],
            "files": [f"bin/{executable}"],
        }
        (package / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
        main["optionalDependencies"][name] = version
        written.append(package)
    package = out / "sqlty"
    shutil.copytree(ROOT / "npm/sqlty", package)
    shutil.copy2(ROOT / "LICENSE", package / "LICENSE")
    main["version"] = version
    (package / "package.json").write_text(json.dumps(main, indent=2) + "\n")
    return [*written, package]


def homebrew(version: str, tarballs: Path) -> str:
    """The Homebrew formula of the release ``version``."""
    stanzas = {}
    for target, (platform, arch) in TARGETS.items():
        if platform == "win32":
            continue
        name = f"cli-{platform}-{arch}"
        tarball = tarballs / f"sqlty-{name}-{version}.tgz"
        sha256 = hashlib.sha256(tarball.read_bytes()).hexdigest()
        url = f"{REGISTRY}/@sqlty/{name}/-/{name}-{version}.tgz"
        stanzas[target] = f'      url "{url}"\n      sha256 "{sha256}"'
    return FORMULA.format(version=version, **stanzas)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    for_npm = commands.add_parser("npm", help="write the npm packages")
    for_npm.add_argument("version")
    for_npm.add_argument("binaries", type=Path)
    for_npm.add_argument("out", type=Path)
    for_brew = commands.add_parser("homebrew", help="print the Homebrew formula")
    for_brew.add_argument("version")
    for_brew.add_argument("tarballs", type=Path)
    args = parser.parse_args()
    version = args.version.removeprefix("v")
    if args.command == "npm":
        for package in npm(version, args.binaries, args.out):
            print(package)
    else:
        sys.stdout.write(homebrew(version, args.tarballs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
