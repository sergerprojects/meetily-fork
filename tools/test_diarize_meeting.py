"""Run: python3 -m unittest tools/test_diarize_meeting.py  (needs no audio or models)"""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("dm", Path(__file__).with_name("diarize_meeting.py"))
dm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dm)


def seg(a, b, who):
    return {"audio_start_time": a, "audio_end_time": b, "speaker": who}


class LabelSegments(unittest.TestCase):
    def test_you_and_two_other_voices(self):
        segs = [seg(0, 5, "Others"), seg(5, 10, "You"), seg(10, 16, "Others"), seg(16, 20, "You"), seg(20, 26, "Others")]
        turns = [  # voice 0 = you (sits under You lines), 1 and 2 = others
            {"start": 0, "end": 5, "speaker": 1}, {"start": 5, "end": 10, "speaker": 0},
            {"start": 10, "end": 16, "speaker": 2}, {"start": 16, "end": 20, "speaker": 0},
            {"start": 20, "end": 26, "speaker": 1}]
        labels = dm.label_segments(segs, turns)
        self.assertEqual(labels["0.00"], "Speaker 1")
        self.assertEqual(labels["5.00"], "You")
        self.assertEqual(labels["10.00"], "Speaker 2")
        self.assertEqual(labels["20.00"], "Speaker 1", "same voice keeps its number")

    def test_short_voice_stays_others(self):
        segs = [seg(0, 6, "You"), seg(6, 8, "Others")]
        turns = [{"start": 0, "end": 6, "speaker": 0}, {"start": 6, "end": 8, "speaker": 1}]
        self.assertEqual(dm.label_segments(segs, turns)["6.00"], "Others")

    def test_no_diarization_overlap_stays_others(self):
        segs = [seg(0, 6, "You"), seg(30, 40, "Others")]
        turns = [{"start": 0, "end": 6, "speaker": 0}]
        self.assertEqual(dm.label_segments(segs, turns)["30.00"], "Others")


if __name__ == "__main__":
    unittest.main()
