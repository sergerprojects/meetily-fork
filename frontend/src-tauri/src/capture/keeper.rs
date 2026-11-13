//! Decides which captured frames of a shared screen are worth keeping.
//!
//! A frame is kept when it looks different from the last kept one, has stopped
//! changing for a moment (so a slide mid-transition is skipped), enough time has
//! passed since the last keep, and the per-meeting cap is not reached.

use super::frame::hamming;

#[derive(Clone, Copy, Debug)]
pub struct KeepRules {
    /// Hash distance at or below which two frames count as the same picture.
    pub same_within: u32,
    /// How long a new picture must stay put before it is kept.
    pub settle_secs: f64,
    /// Minimum time between two kept pictures.
    pub min_gap_secs: f64,
    pub max_images: usize,
}

impl Default for KeepRules {
    fn default() -> Self {
        Self { same_within: 6, settle_secs: 1.5, min_gap_secs: 3.0, max_images: 60 }
    }
}

#[derive(Debug, Default)]
pub struct Keeper {
    rules: KeepRules,
    last_kept: Option<u64>,
    last_kept_at: f64,
    candidate: Option<(u64, f64)>,
    kept: usize,
}

impl Keeper {
    pub fn new(rules: KeepRules) -> Self {
        Self { rules, ..Default::default() }
    }

    pub fn kept(&self) -> usize {
        self.kept
    }

    /// Offer the hash of the frame seen at `t` seconds. True = save this frame.
    pub fn offer(&mut self, hash: u64, t: f64) -> bool {
        if self.kept >= self.rules.max_images {
            return false;
        }
        if let Some(last) = self.last_kept {
            if hamming(hash, last) <= self.rules.same_within {
                self.candidate = None; // back to what we already have
                return false;
            }
        }
        match self.candidate {
            Some((c, since)) if hamming(hash, c) <= self.rules.same_within => {
                let settled = t - since >= self.rules.settle_secs;
                let spaced = self.last_kept.is_none() || t - self.last_kept_at >= self.rules.min_gap_secs;
                if settled && spaced {
                    self.last_kept = Some(hash);
                    self.last_kept_at = t;
                    self.candidate = None;
                    self.kept += 1;
                    true
                } else {
                    false
                }
            }
            _ => {
                self.candidate = Some((hash, t));
                false
            }
        }
    }

    /// Forget the previous picture (a new share started).
    pub fn reset_picture(&mut self) {
        self.last_kept = None;
        self.candidate = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u64 = 0x0000_0000_FFFF_FFFF;
    const B: u64 = 0xFFFF_FFFF_0000_0000;
    const C: u64 = 0x0F0F_0F0F_0F0F_0F0F;

    #[test]
    fn keeps_a_picture_only_after_it_settles() {
        let mut k = Keeper::new(KeepRules::default());
        assert!(!k.offer(A, 0.0), "first sighting is only a candidate");
        assert!(!k.offer(A, 1.0), "not settled yet");
        assert!(k.offer(A, 1.6));
        assert!(!k.offer(A, 2.0), "same picture again");
        assert_eq!(k.kept(), 1);
    }

    #[test]
    fn a_flicker_that_never_settles_is_skipped() {
        let mut k = Keeper::new(KeepRules::default());
        for (i, h) in [A, B, A, B, A, B].into_iter().enumerate() {
            assert!(!k.offer(h, i as f64));
        }
        assert_eq!(k.kept(), 0);
    }

    #[test]
    fn next_slide_is_kept_after_the_minimum_gap() {
        let mut k = Keeper::new(KeepRules::default());
        k.offer(A, 0.0);
        assert!(k.offer(A, 2.0));
        k.offer(B, 2.5);
        assert!(!k.offer(B, 4.0), "settled but only 2s since the last keep");
        assert!(k.offer(B, 5.1));
        assert_eq!(k.kept(), 2);
    }

    #[test]
    fn returning_to_an_older_slide_counts_as_new() {
        let mut k = Keeper::new(KeepRules::default());
        k.offer(A, 0.0);
        k.offer(A, 2.0);
        k.offer(B, 10.0);
        assert!(k.offer(B, 12.0));
        k.offer(A, 20.0);
        assert!(k.offer(A, 22.0), "A again after B is a different moment");
    }

    #[test]
    fn stops_at_the_cap() {
        let rules = KeepRules { max_images: 2, ..KeepRules::default() };
        let mut k = Keeper::new(rules);
        k.offer(A, 0.0);
        assert!(k.offer(A, 2.0));
        k.offer(B, 10.0);
        assert!(k.offer(B, 12.0));
        k.offer(C, 20.0);
        assert!(!k.offer(C, 22.0));
        assert_eq!(k.kept(), 2);
    }

    #[test]
    fn reset_lets_the_same_picture_be_kept_again() {
        let mut k = Keeper::new(KeepRules::default());
        k.offer(A, 0.0);
        assert!(k.offer(A, 2.0));
        k.reset_picture();
        k.offer(A, 100.0);
        assert!(k.offer(A, 102.0));
    }
}
