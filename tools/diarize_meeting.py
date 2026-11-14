#!/usr/bin/env python3
"""Split Graphite's "Others" lines into Speaker 1, Speaker 2, ... by voice.

Graphite already labels each transcript line "You" (microphone) or "Others"
(everyone else on the call, system audio). This runs on-device speaker
diarization over the saved meeting audio and refines "Others" into separate
voices. It does not know names: pair it with the meeting's attendee list
(see scripts/import-graphite.py in a downstream setup, or do it yourself).

Needs: Python 3.9+, `pip install sherpa-onnx numpy`, ffmpeg, and two small
model files (run once with --download-models, ~32 MB total from the
k2-fsa/sherpa-onnx GitHub releases). The meeting folder must contain
audio.mp4 (Settings -> Recording -> save audio on) and transcripts.json.

    python3 tools/diarize_meeting.py ~/Movies/graphite-recordings/<meeting>

Writes <meeting>/speakers.json:
    {"segments": {"<audio_start_time, 2dp>": "You" | "Speaker 1" | "Others"}, ...}

Accuracy: reliable for separating you from one or two other voices. With more
voices it can merge similar-sounding people or split one person in two;
treat labels as best guesses. Everything runs locally.
"""
import argparse
import collections
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
import wave
from pathlib import Path

MODELS_DIR = Path(os.environ.get("GRAPHITE_SPEAKER_MODELS", Path.home() / ".graphite" / "speaker-models"))
SEG_URL = ("https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/"
           "sherpa-onnx-pyannote-segmentation-3-0.tar.bz2")
EMB_URL = ("https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/"
           "wespeaker_en_voxceleb_resnet34_LM.onnx")
SEG_REL = "sherpa-onnx-pyannote-segmentation-3-0/model.int8.onnx"
EMB_NAME = "wespeaker_en_voxceleb_resnet34_LM.onnx"

YOU_OVERLAP_FRAC = 0.5    # a voice is "you" if >=50% of its speech sits under You lines...
YOU_OVERLAP_MIN_S = 3.0   # ...and at least this many seconds
MIN_VOICE_S = 4.0         # voices with less total speech are too unreliable: stay "Others"


def download_models(dest: Path):
    import tarfile
    dest.mkdir(parents=True, exist_ok=True)
    emb = dest / EMB_NAME
    if not emb.exists():
        print(f"downloading {EMB_URL}")
        urllib.request.urlretrieve(EMB_URL, emb)
    if not (dest / SEG_REL).exists():
        print(f"downloading {SEG_URL}")
        with tempfile.TemporaryDirectory() as td:
            tgz = Path(td) / "seg.tar.bz2"
            urllib.request.urlretrieve(SEG_URL, tgz)
            with tarfile.open(tgz, "r:bz2") as t:
                t.extractall(dest, filter="data") if sys.version_info >= (3, 12) else t.extractall(dest)
    print(f"models ready in {dest}")


def load_segments(folder: Path):
    raw = json.loads((folder / "transcripts.json").read_text())
    segs = raw if isinstance(raw, list) else raw.get("segments", [])
    return [s for s in segs if s.get("audio_start_time") is not None and s.get("audio_end_time") is not None]


def to_wav16k(audio: Path, out: Path):
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(audio), "-ac", "1", "-ar", "16000", str(out)], check=True)


def diarize(wav: Path, models: Path, threshold: float):
    import numpy as np
    import sherpa_onnx
    cfg = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(models / SEG_REL)),
            num_threads=4),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(models / EMB_NAME), num_threads=4),
        clustering=sherpa_onnx.FastClusteringConfig(num_clusters=-1, threshold=threshold),
        min_duration_on=0.3, min_duration_off=0.5)
    if not cfg.validate():
        raise SystemExit("speaker models missing or invalid; run with --download-models")
    sd = sherpa_onnx.OfflineSpeakerDiarization(cfg)
    with wave.open(str(wav)) as w:
        if w.getframerate() != sd.sample_rate:
            raise SystemExit(f"unexpected sample rate {w.getframerate()}")
        samples = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
    return [{"start": r.start, "end": r.end, "speaker": r.speaker}
            for r in sd.process(samples).sort_by_start_time()]


def overlap(a0, a1, b0, b1):
    return max(0.0, min(a1, b1) - max(a0, b0))


def label_segments(segs, turns):
    """Return {key: label}. Pure function (unit-testable without audio)."""
    total = collections.Counter()
    under_you = collections.Counter()
    you_spans = [(s["audio_start_time"], s["audio_end_time"]) for s in segs if s.get("speaker") == "You"]
    for t in turns:
        dur = t["end"] - t["start"]
        total[t["speaker"]] += dur
        under_you[t["speaker"]] += sum(overlap(t["start"], t["end"], a, b) for a, b in you_spans)

    you_voices = {v for v in total
                  if under_you[v] >= YOU_OVERLAP_MIN_S and under_you[v] / total[v] >= YOU_OVERLAP_FRAC}
    other_voices = {v for v in total if v not in you_voices and total[v] >= MIN_VOICE_S}

    labels, order = {}, []
    for s in segs:
        key = f"{s['audio_start_time']:.2f}"
        if s.get("speaker") == "You":
            labels[key] = "You"
            continue
        votes = collections.Counter()
        for t in turns:
            if t["speaker"] in other_voices:
                votes[t["speaker"]] += overlap(s["audio_start_time"], s["audio_end_time"], t["start"], t["end"])
        if not votes or votes.most_common(1)[0][1] <= 0:
            labels[key] = "Others"
            continue
        voice = votes.most_common(1)[0][0]
        if voice not in order:
            order.append(voice)
        labels[key] = f"Speaker {order.index(voice) + 1}"
    return labels


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder", nargs="?", help="meeting folder containing audio.mp4 + transcripts.json")
    ap.add_argument("--models", default=str(MODELS_DIR))
    ap.add_argument("--threshold", type=float, default=0.5,
                    help="clustering threshold: lower = more distinct voices (default 0.5)")
    ap.add_argument("--download-models", action="store_true", help="fetch the two model files, then exit if no folder")
    a = ap.parse_args()

    models = Path(a.models).expanduser()
    if a.download_models:
        download_models(models)
    if not a.folder:
        return 0
    folder = Path(a.folder).expanduser()
    audio = folder / "audio.mp4"
    if not audio.exists():
        print(f"no audio.mp4 in {folder} (turn on Settings -> Recording -> save audio)", file=sys.stderr)
        return 2
    segs = load_segments(folder)
    with tempfile.TemporaryDirectory() as td:
        wav = Path(td) / "a.wav"
        to_wav16k(audio, wav)
        turns = diarize(wav, models, a.threshold)
    labels = label_segments(segs, turns)
    (folder / "speakers.json").write_text(json.dumps({
        "method": "sherpa-onnx pyannote-segmentation-3-0 + wespeaker resnet34",
        "threshold": a.threshold, "segments": labels}, indent=1))
    counts = collections.Counter(labels.values())
    print(f"{folder.name}: {dict(sorted(counts.items()))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
