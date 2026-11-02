"""The talk to record, from TALK: cicd (default, ../README.md) or types (../types/README.md)."""

import importlib
import os
from pathlib import Path

HERE = Path(__file__).parent
NAME = os.environ.get("TALK", "cicd")
SCREENPLAYS = {"cicd": "screenplay", "types": "screenplay_types"}
if NAME not in SCREENPLAYS:
    raise SystemExit(f"TALK={NAME}: choose one of {', '.join(SCREENPLAYS)}")
SCENES = importlib.import_module(SCREENPLAYS[NAME]).SCENES
BUILD = HERE / "build" / NAME
OWN_VOICE = HERE / "voice" / NAME
