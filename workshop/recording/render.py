"""Turn build/session.cast and the narration into build/duckdesk.mp4 and build/duckdesk.srt.

    uv run python render.py --agg agg

Silence on screen and in the voice (a build, a slow install) is cut to --idle seconds;
the narration is placed on the cut timeline, so it stays in sync.
"""

import argparse
import array
import bisect
import json
import subprocess
import wave
from pathlib import Path

HERE = Path(__file__).parent
BUILD = HERE / "build"
RATE = 22050


def cut_timeline(events: list[float], spoken: list[tuple[float, float]], idle: float):
    """A function from recorded time to video time that shortens quiet gaps to `idle`."""
    points = sorted({0.0, *events, *(t for span in spoken for t in span)})
    mapped = [0.0]
    for a, b in zip(points, points[1:], strict=False):
        middle = (a + b) / 2
        quiet = not any(start <= middle < end for start, end in spoken)
        mapped.append(mapped[-1] + (min(b - a, idle) if quiet else b - a))

    def at(t: float) -> float:
        i = bisect.bisect_right(points, t) - 1
        if i >= len(points) - 1:
            return mapped[-1] + t - points[-1]
        span = points[i + 1] - points[i]
        return mapped[i] + (mapped[i + 1] - mapped[i]) * (t - points[i]) / span

    return at


def srt_time(seconds: float) -> str:
    ms = round(seconds * 1000)
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--agg", default="agg")
    parser.add_argument("--idle", type=float, default=1.5, help="longest quiet gap, in seconds")
    parser.add_argument("--font-size", default="18")
    args = parser.parse_args()

    header, *lines = (BUILD / "session.cast").read_text().splitlines()
    events = [json.loads(line) for line in lines]
    timeline = json.loads((BUILD / "timeline.json").read_text())
    texts = {cue["id"]: cue["text"] for cue in json.loads((BUILD / "cues.json").read_text())}
    spoken = [(cue["start"], cue["start"] + cue["duration"]) for cue in timeline["cues"]]
    at = cut_timeline([event[0] for event in events], spoken, args.idle)

    cut = BUILD / "cut.cast"
    frames = [json.dumps([round(at(t), 4), *rest]) for t, *rest in events]
    frames.append(json.dumps([round(at(timeline["end"]), 4), "o", "\u001b[0m"]))
    cut.write_text("\n".join([header, *frames]) + "\n")
    subprocess.run(
        [
            args.agg,
            "--font-family",
            "JetBrains Mono,DejaVu Sans Mono",
            "--font-size",
            args.font_size,
            "--theme",
            "monokai",
            "--idle-time-limit",
            "36000",
            "--last-frame-duration",
            "3",
            "--fps-cap",
            "15",
            str(cut),
            str(BUILD / "duckdesk.gif"),
        ],
        check=True,
        capture_output=True,
    )

    length = at(timeline["end"]) + 3
    track = array.array("h", bytes(2 * int(length * RATE)))
    subtitles = []
    for number, cue in enumerate(timeline["cues"], 1):
        start = at(cue["start"])
        with wave.open(str(BUILD / "voice" / f"{cue['id']}.wav")) as clip:
            samples = array.array("h", clip.readframes(clip.getnframes()))
        offset = int(start * RATE)
        track[offset : offset + len(samples)] = samples
        end = start + len(samples) / RATE
        subtitles.append(f"{number}\n{srt_time(start)} --> {srt_time(end)}\n{texts[cue['id']]}\n")
    with wave.open(str(BUILD / "narration.wav"), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(track.tobytes())
    (BUILD / "duckdesk.srt").write_text("\n".join(subtitles))

    subprocess.run(
        [
            "ffmpeg",
            "-loglevel",
            "error",
            "-y",
            "-i",
            str(BUILD / "duckdesk.gif"),
            "-i",
            str(BUILD / "narration.wav"),
            "-i",
            str(BUILD / "duckdesk.srt"),
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:s",
            "-vf",
            "scale=1920:-2:flags=lanczos,fps=30",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-c:s",
            "mov_text",
            "-metadata:s:s:0",
            "language=eng",
            "-movflags",
            "+faststart",
            "-shortest",
            str(BUILD / "duckdesk.mp4"),
        ],
        check=True,
    )
    print(f"build/duckdesk.mp4: {length / 60:.1f} minutes")


if __name__ == "__main__":
    main()
