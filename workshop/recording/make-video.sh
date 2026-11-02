#!/usr/bin/env bash
# Record a DuckDesk talk and render it with narration: build/<talk>/duckdesk.mp4 and .srt.
#   TALK=types ./make-video.sh               # the types talk (default: TALK=cicd)
#   PIPER_MODEL=en_US-lessac-medium.onnx ./make-video.sh
#   ./make-video.sh --chapters merge,rolling     # record only these again, then join all
set -euo pipefail
cd "$(dirname "$0")"
: "${PIPER_MODEL:?set PIPER_MODEL to a Piper voice model (.onnx)}"
python3 voice.py --model "$PIPER_MODEL" --piper "${PIPER:-piper}"
python3 record.py --nvim "${NVIM:-nvim}" --asciinema "${ASCIINEMA:-asciinema}" "$@"
python3 render.py --agg "${AGG:-agg}"
