"""Install a published sqlty release from one source and run it, as a user would.

    python scripts/verify_release.py pypi 0.8.0 python3.14 python3.15t
    python scripts/verify_release.py npm 0.8.0
    python scripts/verify_release.py homebrew 0.8.0

The release workflow runs it on every supported platform once the packages are out.
``pypi`` installs the wheel for each interpreter given (``--only-binary``: a platform
without a wheel fails instead of building the sdist), ``npm`` the ``@sqlty/cli-*`` package
of this platform and ``@sqlty/wasm``, and ``homebrew`` the formula of the tap. Each
installed ``sqlty`` runs ``version``, ``rewrite`` and ``check`` on a small module.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

TAP = "jirikuncar/sqlty"
ATTEMPTS = 10
"""Installs from a registry, 30 seconds apart: its CDN can lag the upload."""
QUERY = 'q = "--sql\\nSELECT 1::INTEGER AS one"\n'
REWRITTEN = 'q = SQL[tuple[int]]("--sql\\nSELECT 1::INTEGER AS one")'
IMPORT = """
import sys, sysconfig, sqlty, sqlty._core
assert sqlty.__version__ == sys.argv[1], sqlty.__version__
# A free-threaded build must keep the GIL off with sqlty imported.
assert sys._is_gil_enabled() != bool(sysconfig.get_config_var("Py_GIL_DISABLED"))
print(sys.version, "sqlty", sqlty.__version__)
"""
WASM = """
import { load } from "@sqlty/wasm";
import manifest from "@sqlty/wasm/package.json" with { type: "json" };
if (manifest.version !== VERSION) throw new Error(`@sqlty/wasm ${manifest.version}`);
const engine = await load();
const answer = engine({ op: "analyze", source: QUERY });
if (!answer.rewritten?.includes(REWRITTEN)) throw new Error(JSON.stringify(answer));
console.log("@sqlty/wasm", manifest.version);
"""
WINDOWS = os.name == "nt"


def shown(args: tuple[str | Path, ...]) -> str:
    """The command as the log shows it: a script given inline as its first line."""
    return " ".join(
        str(arg).strip().splitlines()[0] + " ..." if "\n" in str(arg) else str(arg) for arg in args
    )


def run(*args: str | Path, cwd: Path | None = None) -> str:
    print("$", shown(args), flush=True)
    done = subprocess.run([str(arg) for arg in args], cwd=cwd, capture_output=True, text=True)
    if done.returncode:
        sys.exit(f"exit {done.returncode}\n{done.stdout}{done.stderr}")
    return done.stdout


def install(*args: str | Path, cwd: Path | None = None) -> None:
    """Run an install until the registry serves the release."""
    for attempt in range(1, ATTEMPTS + 1):
        print("$", shown(args), flush=True)
        done = subprocess.run([str(arg) for arg in args], cwd=cwd, capture_output=True, text=True)
        if not done.returncode:
            return
        print(f"attempt {attempt}/{ATTEMPTS}: exit {done.returncode}\n{done.stderr}", flush=True)
        if attempt < ATTEMPTS:
            time.sleep(30)
    sys.exit("the release is not installable")


def tool(name: str) -> str:
    """The path of a command: a list of arguments finds no `.cmd` on Windows otherwise."""
    found = shutil.which(name)
    if found is None:
        sys.exit(f"{name} is not on PATH")
    return found


def smoke(sqlty: Path, version: str, work: Path) -> None:
    """``sqlty`` reports ``version``, rewrites a query and checks the result."""
    found = run(sqlty, "version").strip()
    if found != f"sqlty {version}":
        sys.exit(f"{sqlty} version: {found}")
    module = work / "queries.py"
    module.write_text(QUERY)
    run(sqlty, "rewrite", module)
    if REWRITTEN not in module.read_text():
        sys.exit(f"{sqlty} rewrite:\n{module.read_text()}")
    run(sqlty, "check", module)


def pypi(version: str, pythons: list[str], work: Path) -> None:
    for index, python in enumerate(pythons):
        venv = work / f"venv-{index}"
        run(python, "-m", "venv", venv)
        scripts = venv / ("Scripts" if WINDOWS else "bin")
        venv_python = scripts / ("python.exe" if WINDOWS else "python")
        install(
            venv_python, "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
            "--no-cache-dir", "--only-binary", ":all:", f"sqlty=={version}",
        )  # fmt: skip
        print(run(venv_python, "-c", IMPORT, version), end="")
        smoke(scripts / ("sqlty.exe" if WINDOWS else "sqlty"), version, work)


def npm(version: str, work: Path) -> None:
    node = tool("node")
    platform = run(node, "-p", "`${process.platform}-${process.arch}`").strip()
    package = f"@sqlty/cli-{platform}"
    (work / "package.json").write_text("{}\n")
    install(
        tool("npm"), "install", "--no-audit", "--no-fund",
        f"{package}@{version}", f"@sqlty/wasm@{version}", cwd=work,
    )  # fmt: skip
    executable = "sqlty.exe" if platform.startswith("win32") else "sqlty"
    smoke(work / "node_modules" / package / "bin" / executable, version, work)
    script = WASM
    for name, value in (("VERSION", version), ("QUERY", QUERY), ("REWRITTEN", REWRITTEN)):
        script = script.replace(name, json.dumps(value))
    print(run(node, "--input-type=module", "-e", script, cwd=work), end="")


def homebrew(version: str, work: Path) -> None:
    brew = tool("brew")
    run(brew, "tap", TAP)
    # Recent Homebrew loads the formulae of a third-party tap only once it is trusted.
    if subprocess.run([brew, "trust", "--help"], capture_output=True).returncode == 0:
        run(brew, "trust", TAP)
    run(brew, "install", f"{TAP}/sqlty")
    run(brew, "test", f"{TAP}/sqlty")
    smoke(Path(run(brew, "--prefix").strip()) / "bin" / "sqlty", version, work)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("source", choices=["pypi", "npm", "homebrew"])
    parser.add_argument("version", help="The release, such as 0.8.0")
    parser.add_argument("pythons", nargs="*", help="pypi: the interpreters to install it on")
    args = parser.parse_args()
    if (args.source == "pypi") != bool(args.pythons):
        parser.error("pypi takes the interpreters, and only pypi")
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        if args.source == "pypi":
            pypi(args.version, args.pythons, work)
        elif args.source == "npm":
            npm(args.version, work)
        else:
            homebrew(args.version, work)
    print(f"sqlty {args.version} from {args.source} works")
    return 0


if __name__ == "__main__":
    sys.exit(main())
