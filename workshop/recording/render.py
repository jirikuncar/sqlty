"""Render each recorded chapter with its narration, then join them into build/duckdesk.mp4.

    uv run python render.py --agg agg

A chapter renders again only when its recording is newer than its video (or with --force).
Silence on screen and in the voice (a build, a slow install) is cut to --idle seconds;
the narration is placed on the cut timeline, so it stays in sync.
"""

import argparse
import array
import bisect
import json
import subprocess
import wave
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from screenplay import SCENES

HERE = Path(__file__).parent
BUILD = HERE / "build"
CHAPTERS = BUILD / "chapters"
RATE = 22050
TAIL = 1.0


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


def ffmpeg(*args: str) -> None:
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", *args], check=True)


def seconds(path: Path) -> float:
    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )
    return float(probe.stdout)


def render(name: str, args: argparse.Namespace) -> None:
    """build/chapters/<name>.mp4, and <name>.subs.json with each line's place in it."""
    header, *lines = (CHAPTERS / f"{name}.cast").read_text().splitlines()
    events = [json.loads(line) for line in lines]
    timeline = json.loads((CHAPTERS / f"{name}.json").read_text())
    spoken = [(cue["start"], cue["start"] + cue["duration"]) for cue in timeline["cues"]]
    at = cut_timeline([event[0] for event in events], spoken, args.idle)

    frames = [json.dumps([round(at(t), 4), *rest]) for t, *rest in events]
    frames.append(json.dumps([round(at(timeline["end"]), 4), "o", "\u001b[0m"]))
    cut = CHAPTERS / f"{name}.cut.cast"
    cut.write_text("\n".join([header, *frames]) + "\n")
    gif = CHAPTERS / f"{name}.gif"
    subprocess.run(
        [args.agg, "--font-family", "JetBrains Mono,DejaVu Sans Mono", "--font-size", args.font_size,
         "--theme", "monokai", "--idle-time-limit", "36000", "--last-frame-duration", str(TAIL),
         "--fps-cap", "15", str(cut), str(gif)],
        check=True,
        capture_output=True,
    )  # fmt: skip

    track = array.array("h", bytes(2 * int((at(timeline["end"]) + TAIL) * RATE)))
    subs = []
    for cue in timeline["cues"]:
        start = at(cue["start"])
        with wave.open(str(BUILD / "voice" / f"{cue['id']}.wav")) as clip:
            samples = array.array("h", clip.readframes(clip.getnframes()))
        offset = int(start * RATE)
        track[offset : offset + len(samples)] = samples[: len(track) - offset]
        subs.append({"id": cue["id"], "start": start, "end": start + len(samples) / RATE})
    voice = CHAPTERS / f"{name}.wav"
    with wave.open(str(voice), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(RATE)
        out.writeframes(track.tobytes())

    ffmpeg(
        "-i", str(gif), "-i", str(voice),
        "-vf", "scale=1920:-2:flags=lanczos,fps=30", "-c:v", "libx264", "-pix_fmt", "yuv420p",
        "-af", "apad", "-c:a", "aac", "-ar", "48000", "-b:a", "128k", "-shortest",
        str(CHAPTERS / f"{name}.mp4"),
    )  # fmt: skip
    (CHAPTERS / f"{name}.subs.json").write_text(json.dumps(subs))
    print(f"chapter {name}: {seconds(CHAPTERS / f'{name}.mp4'):.0f} s")


def srt_time(t: float) -> str:
    ms = round(t * 1000)
    return f"{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}"


def join(names: list[str]) -> None:
    """build/duckdesk.mp4 from the chapters, with subtitles and a chapter list."""
    texts = {cue["id"]: cue["text"] for cue in json.loads((BUILD / "cues.json").read_text())}
    (BUILD / "chapters.txt").write_text("".join(f"file 'chapters/{name}.mp4'\n" for name in names))
    meta, subtitles, offset = [";FFMETADATA1"], [], 0.0
    for name in names:
        length = seconds(CHAPTERS / f"{name}.mp4")
        meta += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={round(offset * 1000)}",
                 f"END={round((offset + length) * 1000)}", f"title={name}"]  # fmt: skip
        for cue in json.loads((CHAPTERS / f"{name}.subs.json").read_text()):
            subtitles.append(
                f"{len(subtitles) + 1}\n{srt_time(offset + cue['start'])} --> "
                f"{srt_time(offset + cue['end'])}\n{texts[cue['id']]}\n"
            )
        offset += length
    (BUILD / "chapters.meta").write_text("\n".join(meta) + "\n")
    (BUILD / "duckdesk.srt").write_text("\n".join(subtitles))
    ffmpeg("-f", "concat", "-safe", "0", "-i", str(BUILD / "chapters.txt"), "-c", "copy",
           str(BUILD / "joined.mp4"))  # fmt: skip
    ffmpeg(
        "-i", str(BUILD / "joined.mp4"), "-i", str(BUILD / "duckdesk.srt"),
        "-i", str(BUILD / "chapters.meta"),
        "-map", "0", "-map", "1", "-map_chapters", "2", "-c", "copy", "-c:s", "mov_text",
        "-metadata:s:s:0", "language=eng", "-movflags", "+faststart", str(BUILD / "duckdesk.mp4"),
    )  # fmt: skip
    print(f"build/duckdesk.mp4: {offset / 60:.1f} minutes, {len(names)} of {len(SCENES)} chapters")


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--agg", default="agg")
    parser.add_argument("--idle", type=float, default=1.5, help="longest quiet gap, in seconds")
    parser.add_argument("--font-size", default="18")
    parser.add_argument("--force", action="store_true", help="render every chapter again")
    args = parser.parse_args()

    recorded = [s.name for s in SCENES if (CHAPTERS / f"{s.name}.json").exists()]
    stale = [
        name
        for name in recorded
        if args.force
        or not (video := CHAPTERS / f"{name}.mp4").exists()
        or video.stat().st_mtime < (CHAPTERS / f"{name}.json").stat().st_mtime
    ]
    with ThreadPoolExecutor() as pool:
        list(pool.map(lambda name: render(name, args), stale))
    join(recorded)


if __name__ == "__main__":
    main()
