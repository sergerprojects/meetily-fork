//! A captured picture of a window, plus the small image operations snapshots need:
//! crop, a tiny perceptual hash for "did the slide change?", and PNG encoding.

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub w: usize,
    pub h: usize,
    /// Tightly packed B, G, R, A bytes (the order macOS hands out).
    pub bgra: Vec<u8>,
}

impl Frame {
    pub fn new(w: usize, h: usize, bgra: Vec<u8>) -> Option<Self> {
        (w > 0 && h > 0 && bgra.len() == w * h * 4).then_some(Self { w, h, bgra })
    }

    /// Copy of the rectangle (clamped to the picture). `None` if nothing is left.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Option<Frame> {
        let x0 = x.min(self.w);
        let y0 = y.min(self.h);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut out = Vec::with_capacity(cw * ch * 4);
        for row in y0..y1 {
            let start = (row * self.w + x0) * 4;
            out.extend_from_slice(&self.bgra[start..start + cw * 4]);
        }
        Frame::new(cw, ch, out)
    }

    fn luma(&self, x: usize, y: usize) -> f32 {
        let i = (y * self.w + x) * 4;
        0.114 * self.bgra[i] as f32 + 0.587 * self.bgra[i + 1] as f32 + 0.299 * self.bgra[i + 2] as f32
    }

    /// 64-bit average hash: shrink to 8x8 blocks of brightness, bit set when a
    /// block is brighter than the mean. Similar pictures differ in few bits.
    pub fn ahash(&self) -> u64 {
        let mut cells = [0f32; 64];
        for by in 0..8 {
            for bx in 0..8 {
                let (x0, x1) = (bx * self.w / 8, ((bx + 1) * self.w / 8).max(bx * self.w / 8 + 1).min(self.w));
                let (y0, y1) = (by * self.h / 8, ((by + 1) * self.h / 8).max(by * self.h / 8 + 1).min(self.h));
                let mut sum = 0f32;
                let mut n = 0f32;
                // Sample at most ~16x16 pixels per block; plenty for a hash.
                let sx = ((x1 - x0) / 16).max(1);
                let sy = ((y1 - y0) / 16).max(1);
                let mut y = y0;
                while y < y1 {
                    let mut x = x0;
                    while x < x1 {
                        sum += self.luma(x, y);
                        n += 1.0;
                        x += sx;
                    }
                    y += sy;
                }
                cells[by * 8 + bx] = if n > 0.0 { sum / n } else { 0.0 };
            }
        }
        let mean = cells.iter().sum::<f32>() / 64.0;
        cells.iter().enumerate().fold(0u64, |h, (i, c)| if *c > mean { h | (1 << i) } else { h })
    }

    /// Share of pixels (sampled) that are not close to one flat colour. Used to
    /// skip blank or black frames.
    pub fn is_blank(&self) -> bool {
        let mut min = 255f32;
        let mut max = 0f32;
        let step_y = (self.h / 32).max(1);
        let step_x = (self.w / 32).max(1);
        let mut y = 0;
        while y < self.h {
            let mut x = 0;
            while x < self.w {
                let l = self.luma(x, y);
                min = min.min(l);
                max = max.max(l);
                x += step_x;
            }
            y += step_y;
        }
        max - min < 12.0
    }

    /// Bounding box (x, y, w, h) of the dense, non-dark content, ignoring a dark
    /// background and sparse interface bits (text, buttons) around it. Rows and columns
    /// count only when at least `frac` of their pixels are lighter than `dark`.
    /// None when there is no such block or it is under 5% of the picture.
    pub fn dense_bounds(&self, dark: f32, frac: f32) -> Option<(usize, usize, usize, usize)> {
        const S: usize = 2; // sample every 2nd pixel
        let gw = self.w / S;
        let gh = self.h / S;
        if gw < 4 || gh < 4 {
            return None;
        }
        let bright: Vec<bool> = (0..gh).flat_map(|gy| (0..gw).map(move |gx| (gx, gy))).map(|(gx, gy)| self.luma(gx * S, gy * S) > dark).collect();
        let at = |gx: usize, gy: usize| bright[gy * gw + gx];
        let span = |lo: usize, hi: usize, lines: usize, count: &dyn Fn(usize) -> usize, across: usize| -> Option<(usize, usize)> {
            let _ = (lo, hi);
            let ok: Vec<usize> = (0..lines).filter(|i| count(*i) as f32 / across.max(1) as f32 >= frac).collect();
            Some((*ok.first()?, *ok.last()?))
        };
        // Pass 1: rows over the full width, then columns over those rows, then rows again over those columns.
        let (mut y0, mut y1) = span(0, gw, gh, &|gy| (0..gw).filter(|gx| at(*gx, gy)).count(), gw)?;
        let (x0, x1) = span(y0, y1, gw, &|gx| (y0..=y1).filter(|gy| at(gx, *gy)).count(), y1 - y0 + 1)?;
        let (ny0, ny1) = span(x0, x1, gh, &|gy| (x0..=x1).filter(|gx| at(*gx, gy)).count(), x1 - x0 + 1)?;
        y0 = ny0;
        y1 = ny1;
        let (x, y, w, h) = (x0 * S, y0 * S, (x1 - x0 + 1) * S, (y1 - y0 + 1) * S);
        (w * h * 20 >= self.w * self.h).then_some((x, y, w, h))
    }

    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut rgba = Vec::with_capacity(self.bgra.len());
        for px in self.bgra.chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.w as u32, self.h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(&rgba).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, b: u8, g: u8, r: u8) -> Frame {
        let mut v = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            v.extend_from_slice(&[b, g, r, 255]);
        }
        Frame::new(w, h, v).unwrap()
    }

    /// Left half black, right half white.
    fn split(w: usize, h: usize, left_dark: bool) -> Frame {
        let mut v = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let dark = (x < w / 2) == left_dark;
                let c = if dark { 10 } else { 245 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        Frame::new(w, h, v).unwrap()
    }

    #[test]
    fn rejects_wrong_sizes() {
        assert!(Frame::new(2, 2, vec![0; 15]).is_none());
        assert!(Frame::new(0, 2, vec![]).is_none());
    }

    #[test]
    fn crop_copies_the_right_pixels_and_clamps() {
        let f = split(8, 4, true);
        let right = f.crop(4, 0, 4, 4).unwrap();
        assert_eq!((right.w, right.h), (4, 4));
        assert!(right.bgra.chunks(4).all(|p| p[0] == 245));
        let clamped = f.crop(6, 2, 100, 100).unwrap();
        assert_eq!((clamped.w, clamped.h), (2, 2));
        assert!(f.crop(8, 0, 3, 3).is_none());
    }

    #[test]
    fn hash_is_stable_and_tells_pictures_apart() {
        let a = split(160, 90, true);
        let a2 = split(160, 90, true);
        let b = split(160, 90, false);
        assert_eq!(a.ahash(), a2.ahash());
        assert!(hamming(a.ahash(), b.ahash()) > 20, "mirrored picture must look different");
    }

    #[test]
    fn small_noise_barely_moves_the_hash() {
        let a = split(160, 90, true);
        let mut b = a.clone();
        for i in (0..b.bgra.len()).step_by(4 * 97) {
            b.bgra[i] = b.bgra[i].saturating_add(6);
        }
        assert!(hamming(a.ahash(), b.ahash()) <= 4);
    }

    #[test]
    fn blank_frames_are_detected() {
        assert!(solid(64, 64, 0, 0, 0).is_blank());
        assert!(solid(64, 64, 200, 200, 200).is_blank());
        assert!(!split(64, 64, true).is_blank());
    }

    /// Black picture with a white block and a little bright "interface" noise outside it.
    fn page_with_block() -> Frame {
        let (w, h) = (200usize, 120usize);
        let mut v = vec![0u8; w * h * 4];
        let mut set = |x: usize, y: usize, c: u8| {
            let i = (y * w + x) * 4;
            v[i] = c;
            v[i + 1] = c;
            v[i + 2] = c;
            v[i + 3] = 255;
        };
        for y in 20..90 {
            for x in 40..170 {
                set(x, y, 235);
            }
        }
        for x in 60..140 {
            set(x, 5, 255); // thin bright line (like a title bar)
        }
        for y in 100..112 {
            for x in 80..110 {
                set(x, y, 255); // small button
            }
        }
        Frame::new(w, h, v).unwrap()
    }

    #[test]
    fn dense_bounds_finds_the_content_and_ignores_sparse_interface() {
        let (x, y, w, h) = page_with_block().dense_bounds(40.0, 0.4).unwrap();
        assert!((38..=42).contains(&x) && (18..=22).contains(&y), "origin {x},{y}");
        assert!((126..=134).contains(&w) && (66..=74).contains(&h), "size {w}x{h}");
    }

    #[test]
    fn dense_bounds_of_a_dark_picture_is_none() {
        assert!(solid(64, 64, 0, 0, 0).dense_bounds(40.0, 0.4).is_none());
    }

    #[test]
    fn dense_bounds_of_a_bright_picture_is_all_of_it() {
        let (x, y, w, h) = solid(64, 48, 240, 240, 240).dense_bounds(40.0, 0.4).unwrap();
        assert_eq!((x, y), (0, 0));
        assert!(w >= 62 && h >= 46);
    }

    #[test]
    fn png_has_the_signature_and_decodes_to_the_same_size() {
        let f = split(16, 8, true);
        let bytes = f.to_png().unwrap();
        assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let dec = png::Decoder::new(std::io::Cursor::new(bytes));
        let r = dec.read_info().unwrap();
        assert_eq!((r.info().width, r.info().height), (16, 8));
    }
}
