#!/usr/bin/env bash
# Optional: enable live "who is speaking" labels (Speaker 1, 2, ...) for the call side.
# Creates ~/.meetily/venv with sherpa-onnx and downloads the voice model (~26 MB, from the
# k2-fsa/sherpa-onnx GitHub releases). Audio is analysed in memory only and never saved.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v python3 >/dev/null || { echo "python3 is required"; exit 1; }
[ -x "$HOME/.meetily/venv/bin/python" ] || python3 -m venv "$HOME/.meetily/venv"
"$HOME/.meetily/venv/bin/pip" install -q sherpa-onnx numpy
"$HOME/.meetily/venv/bin/python" "$ROOT/tools/diarize_meeting.py" --download-models
echo "Live speaker labels are ready. Restart Graphite; they start with the next recording."
