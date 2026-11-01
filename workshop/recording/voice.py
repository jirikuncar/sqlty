"""Narration audio, one file per line: your own take if there is one, else Piper.

    uv run --with piper-tts python voice.py --model en_US-lessac-medium.onnx

Your takes go in voice/own/<id>.wav (any format ffmpeg reads); build/cue-sheet.md lists the ids.
"""

import argparse
import hashlib
import json
import subprocess
import wave
from pathlib import Path

from kit import Say, spoken
from screenplay import SCENES

HERE = Path(__file__).parent
BUILD = HERE / "build"
RATE = 22050


def duration(path: Path) -> float:
    with wave.open(str(path)) as audio:
        return audio.getnframes() / audio.getframerate()


def normalize(source: Path, target: Path) -> None:
    subprocess.run(
        [
            "ffmpeg",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(source),
            "-ac",
            "1",
            "-ar",
            str(RATE),
            "-sample_fmt",
            "s16",
            str(target),
        ],
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--model", required=True, help="Piper voice model (.onnx)")
    parser.add_argument("--piper", default="piper", help="Piper executable")
    parser.add_argument("--length-scale", default="1.1", help="above 1 speaks slower")
    args = parser.parse_args()

    lines = [action for scene in SCENES for action in scene.actions if isinstance(action, Say)]
    (BUILD / "voice").mkdir(parents=True, exist_ok=True)
    (BUILD / "piper").mkdir(exist_ok=True)
    cues = []
    for line in lines:
        target = BUILD / "voice" / f"{line.id}.wav"
        own = next((HERE / "voice" / "own").glob(f"{line.id}.*"), None)
        if own:
            normalize(own, target)
        else:
            text = spoken(line.text)
            cached = (
                BUILD / "piper" / f"{line.id}-{hashlib.sha1(text.encode()).hexdigest()[:8]}.wav"
            )
            if not cached.exists():
                raw = cached.with_suffix(".raw.wav")
                subprocess.run(
                    [
                        args.piper,
                        "-m",
                        args.model,
                        "--length-scale",
                        args.length_scale,
                        "-f",
                        str(raw),
                    ],
                    input=text.encode(),
                    check=True,
                    capture_output=True,
                )
                normalize(raw, cached)
                raw.unlink()
            target.write_bytes(cached.read_bytes())
        cues.append(
            {
                "id": line.id,
                "text": line.text,
                "duration": round(duration(target), 3),
                "own": bool(own),
            }
        )

    (BUILD / "cues.json").write_text(json.dumps(cues, indent=2))
    sheet = [
        "# Cue sheet",
        "",
        "Record each line as `voice/own/<id>.wav`, then run `make-video.sh` again.",
        "",
    ]
    sheet += [f"- `{cue['id']}` ({cue['duration']:.1f} s): {cue['text']}" for cue in cues]
    (BUILD / "cue-sheet.md").write_text("\n".join(sheet) + "\n")
    total = sum(cue["duration"] for cue in cues)
    print(
        f"{len(cues)} lines, {total / 60:.1f} minutes of narration, {sum(c['own'] for c in cues)} of them yours"
    )


if __name__ == "__main__":
    main()
