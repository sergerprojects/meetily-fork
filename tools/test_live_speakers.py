"""Run: python3 -m unittest tools/test_live_speakers.py  (pure logic; no audio or model needed)"""
import importlib.util
import sys
import types
import unittest
from pathlib import Path

import numpy as np

sys.modules.setdefault("sherpa_onnx", types.ModuleType("sherpa_onnx"))
spec = importlib.util.spec_from_file_location("ls", Path(__file__).with_name("live_speakers.py"))
ls = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ls)


def vec(i, noise=0.0, dim=64, seed=0):
    rng = np.random.default_rng(seed)
    base = np.zeros(dim)
    base[i] = 1.0
    return base + rng.normal(0, noise, dim)


class Clustering(unittest.TestCase):
    def test_same_voice_keeps_its_number_and_new_voices_get_new_numbers(self):
        c = ls.Clusterer(assign_sim=0.5)
        a1 = c.assign(vec(0, 0.05, seed=1), 3)[0]
        b1 = c.assign(vec(1, 0.05, seed=2), 3)[0]
        a2 = c.assign(vec(0, 0.05, seed=3), 3)[0]
        b2 = c.assign(vec(1, 0.05, seed=4), 3)[0]
        c1 = c.assign(vec(2, 0.05, seed=5), 3)[0]
        self.assertEqual((a1, b1, a2, b2, c1), (0, 1, 0, 1, 2))

    def test_short_audio_never_creates_a_speaker(self):
        c = ls.Clusterer()
        self.assertEqual(c.assign(vec(0), 0.8)[0], -1)
        self.assertEqual(len(c.centroids), 0)

    def test_short_audio_attaches_only_to_a_clear_match(self):
        c = ls.Clusterer(assign_sim=0.5)
        c.assign(vec(0), 3)
        self.assertEqual(c.assign(vec(0), 0.9)[0], 0)
        self.assertEqual(c.assign(vec(1), 0.9)[0], -1)

    def test_medium_audio_cannot_start_a_new_speaker(self):
        c = ls.Clusterer(assign_sim=0.5)
        c.assign(vec(0), 3)
        self.assertEqual(c.assign(vec(1), 2.0)[0], -1, "a 2 s stretch that matches nobody is not enough to add a voice")
        self.assertEqual(len(c.centroids), 1)
        self.assertEqual(c.assign(vec(1), 3.0)[0], 1)

    def test_speaker_cap(self):
        c = ls.Clusterer(max_speakers=2)
        for i in range(5):
            c.assign(vec(i), 3)
        self.assertEqual(len(c.centroids), 2)


if __name__ == "__main__":
    unittest.main()
