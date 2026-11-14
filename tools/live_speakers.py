#!/usr/bin/env python3
"""Live "who is speaking" labels for Graphite, entirely in memory.

Graphite starts this once per recording and sends it each stretch of call audio (the
"Others" side) as it is transcribed. It answers with a speaker number: the same voice
gets the same number for the whole meeting. No audio is ever written to disk.

Protocol (binary on stdin, JSON lines on stdout):
  in : uint32 n_samples, uint32 sample_rate, then n_samples float32 (little endian)
  out: {"ready": true}  once, then per request  {"speaker": <0..>|-1, "sim": <0..1>}
  speaker -1 = too short/unclear to say.

Needs `pip install sherpa-onnx numpy` and the voice model from
`tools/diarize_meeting.py --download-models` (wespeaker_en_voxceleb_resnet34_LM.onnx).
"""
import json
import os
import struct
import sys
from pathlib import Path

import numpy as np

MODELS = Path(os.environ.get("GRAPHITE_SPEAKER_MODELS", Path.home() / ".graphite" / "speaker-models"))
MODEL_FILE = "wespeaker_en_voxceleb_resnet34_LM.onnx"
TARGET_SR = 16000
MIN_SECONDS = 1.2           # shorter stretches are too unreliable to label
CREATE_MIN_SECONDS = 2.5    # only a longer stretch may start a NEW speaker (short ones split one voice in two)
CENTROID_MIN_SECONDS = 2.0  # only longer stretches move a voice's average
ASSIGN_SIM = float(os.environ.get("GRAPHITE_SPEAKER_SIM", "0.84"))
LOOSE_MARGIN = 0.10         # a shorter stretch may join a voice this much below the strict bar
MAX_SPEAKERS = 12


class Clusterer:
    """Online clustering of voice embeddings by cosine similarity (pure logic, testable)."""

    def __init__(self, assign_sim=ASSIGN_SIM, max_speakers=MAX_SPEAKERS):
        self.assign_sim = assign_sim
        self.max_speakers = max_speakers
        self.centroids: list[np.ndarray] = []
        self.counts: list[int] = []

    @staticmethod
    def _unit(v):
        v = np.asarray(v, dtype=np.float64)
        n = np.linalg.norm(v)
        return v / n if n > 0 else v

    def assign(self, emb, seconds):
        """Return (speaker, similarity); speaker -1 when the audio is too short to place."""
        e = self._unit(emb)
        best, best_sim = -1, 0.0
        for i, c in enumerate(self.centroids):
            s = float(np.dot(e, c))
            if s > best_sim:
                best, best_sim = i, s
        if seconds < MIN_SECONDS:
            # Very short: only attach to a clearly matching known voice.
            return (best, best_sim) if best_sim >= self.assign_sim else (-1, best_sim)
        if best >= 0 and best_sim >= self.assign_sim:
            if seconds >= CENTROID_MIN_SECONDS:
                n = self.counts[best]
                self.centroids[best] = self._unit(self.centroids[best] * n + e)
                self.counts[best] = n + 1
            return best, best_sim
        if seconds < CREATE_MIN_SECONDS:
            # Not long enough to start a new voice: join the closest one if it is close enough.
            return (best, best_sim) if best >= 0 and best_sim >= self.assign_sim - LOOSE_MARGIN else (-1, best_sim)
        if len(self.centroids) >= self.max_speakers:
            return best, best_sim
        self.centroids.append(e)
        self.counts.append(1)
        return len(self.centroids) - 1, best_sim


def resample(x, sr):
    if sr == TARGET_SR or len(x) == 0:
        return x
    n = int(len(x) * TARGET_SR / sr)
    return np.interp(np.linspace(0, len(x) - 1, n), np.arange(len(x)), x).astype(np.float32)


def main():
    import sherpa_onnx
    model = MODELS / MODEL_FILE
    if not model.exists():
        print(json.dumps({"error": f"voice model missing: {model}"}), flush=True)
        return 2
    cfg = sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(model), num_threads=2)
    extractor = sherpa_onnx.SpeakerEmbeddingExtractor(cfg)
    clusterer = Clusterer()
    print(json.dumps({"ready": True}), flush=True)
    stdin = sys.stdin.buffer
    while True:
        head = stdin.read(8)
        if len(head) < 8:
            return 0
        n, sr = struct.unpack("<II", head)
        raw = stdin.read(n * 4)
        if len(raw) < n * 4:
            return 0
        samples = resample(np.frombuffer(raw, dtype=np.float32), sr)
        seconds = len(samples) / TARGET_SR
        if seconds < 0.6:
            print(json.dumps({"speaker": -1, "sim": 0.0}), flush=True)
            continue
        stream = extractor.create_stream()
        stream.accept_waveform(sample_rate=TARGET_SR, waveform=samples)
        stream.input_finished()
        emb = np.array(extractor.compute(stream), dtype=np.float32)
        speaker, sim = clusterer.assign(emb, seconds)
        print(json.dumps({"speaker": int(speaker), "sim": round(float(sim), 3)}), flush=True)


if __name__ == "__main__":
    sys.exit(main())
