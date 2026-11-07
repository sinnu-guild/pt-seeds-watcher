//! Finding a small picture (a seed's icon) inside a screenshot.
//!
//! Uses zero-mean normalized cross-correlation (ZNCC): the score is 1.0 for a
//! perfect match and is not thrown off by small brightness/contrast changes.
//! To stay fast, it scans a shrunk copy of the screenshot first, then checks
//! the best few spots again at full size.

use std::collections::HashMap;

/// A grayscale image with one f32 brightness value (0-255) per pixel.
#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
}

impl Gray {
    pub fn from_rgba(w: usize, h: usize, rgba: &[u8]) -> Gray {
        let px = rgba
            .chunks_exact(4)
            .take(w * h)
            .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
            .collect();
        Gray { w, h, px }
    }

    /// A rectangle cut out of this image, clipped to its edges.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Gray {
        let x0 = x.min(self.w);
        let y0 = y.min(self.h);
        let w = w.min(self.w - x0);
        let h = h.min(self.h - y0);
        let mut px = Vec::with_capacity(w * h);
        for row in y0..y0 + h {
            let start = row * self.w + x0;
            px.extend_from_slice(&self.px[start..start + w]);
        }
        Gray { w, h, px }
    }

    /// Shrinks by an integer factor, averaging each f x f block.
    pub fn downscale(&self, f: usize) -> Gray {
        if f <= 1 {
            return self.clone();
        }
        let (w, h) = (self.w / f, self.h / f);
        let inv = 1.0 / (f * f) as f32;
        let mut px = vec![0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0f32;
                for dy in 0..f {
                    let row = (y * f + dy) * self.w + x * f;
                    for dx in 0..f {
                        sum += self.px[row + dx];
                    }
                }
                px[y * w + x] = sum * inv;
            }
        }
        Gray { w, h, px }
    }

    /// Standard deviation of brightness; near zero means a blank/black image.
    pub fn std_dev(&self) -> f32 {
        if self.px.is_empty() {
            return 0.0;
        }
        let n = self.px.len() as f64;
        let mean = self.px.iter().map(|&v| v as f64).sum::<f64>() / n;
        let var = self.px.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
        var.sqrt() as f32
    }
}

