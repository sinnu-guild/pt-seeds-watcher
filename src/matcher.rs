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

/// Summed-area tables so any rectangle's sum and sum of squares is O(1).
struct Integral {
    stride: usize,
    sum: Vec<f64>,
    sq: Vec<f64>,
}

impl Integral {
    fn new(g: &Gray) -> Integral {
        let stride = g.w + 1;
        let mut sum = vec![0f64; stride * (g.h + 1)];
        let mut sq = vec![0f64; stride * (g.h + 1)];
        for y in 0..g.h {
            let (mut row_sum, mut row_sq) = (0f64, 0f64);
            for x in 0..g.w {
                let v = g.px[y * g.w + x] as f64;
                row_sum += v;
                row_sq += v * v;
                sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row_sum;
                sq[(y + 1) * stride + x + 1] = sq[y * stride + x + 1] + row_sq;
            }
        }
        Integral { stride, sum, sq }
    }

    fn rect(&self, table: &[f64], x: usize, y: usize, w: usize, h: usize) -> f64 {
        let s = self.stride;
        table[(y + h) * s + x + w] - table[y * s + x + w] - table[(y + h) * s + x] + table[y * s + x]
    }
}

/// A template at one scale, with its mean removed ahead of time.
struct Level {
    w: usize,
    h: usize,
    zero_mean: Vec<f32>,
    norm: f32,
}

impl Level {
    fn new(g: &Gray) -> Option<Level> {
        let n = g.px.len() as f32;
        let mean = g.px.iter().sum::<f32>() / n;
        let zero_mean: Vec<f32> = g.px.iter().map(|v| v - mean).collect();
        let norm = zero_mean.iter().map(|v| v * v).sum::<f32>().sqrt();
        // A template with (almost) no detail can't be matched meaningfully.
        if norm < (n.sqrt() * 2.0) {
            return None;
        }
        Some(Level {
            w: g.w,
            h: g.h,
            zero_mean,
            norm,
        })
    }
}

/// A seed picture, ready to be searched for.
pub struct Template {
    pub name: String,
    pub threshold: f32,
    pub w: usize,
    pub h: usize,
    factor: usize,
    full: Level,
    coarse: Level,
}

impl Template {
    pub fn new(name: &str, g: Gray, threshold: f32) -> Result<Template, String> {
        if g.w < 8 || g.h < 8 {
            return Err(format!(
                "the picture for '{name}' is only {}x{} pixels; crop a bigger area around the seed",
                g.w, g.h
            ));
        }
        // Shrink so the coarse template is roughly 16px on its short side.
        let factor = (g.w.min(g.h) / 16).clamp(1, 4);
        let flat = || format!("the picture for '{name}' is almost a single flat colour; crop an area with more detail");
        let full = Level::new(&g).ok_or_else(flat)?;
        let coarse = Level::new(&g.downscale(factor)).ok_or_else(flat)?;
        Ok(Template {
            name: name.to_string(),
            threshold,
            w: g.w,
            h: g.h,
            factor,
            full,
            coarse,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Match {
    pub score: f32,
    pub x: usize,
    pub y: usize,
}

fn zncc_at(img: &Gray, ii: &Integral, t: &Level, x: usize, y: usize) -> f32 {
    let n = (t.w * t.h) as f64;
    let s = ii.rect(&ii.sum, x, y, t.w, t.h);
    let q = ii.rect(&ii.sq, x, y, t.w, t.h);
    let spread = q - s * s / n; // n * variance of the image patch
    if spread <= n {
        return 0.0; // patch is basically flat (std dev < 1 brightness level)
    }
    let mut num = 0f32;
    for j in 0..t.h {
        let start = (y + j) * img.w + x;
        let row = &img.px[start..start + t.w];
        let trow = &t.zero_mean[j * t.w..(j + 1) * t.w];
        num += row.iter().zip(trow).map(|(a, b)| a * b).sum::<f32>();
    }
    (num as f64 / (t.norm as f64 * spread.sqrt())) as f32
}

/// One screenshot prepared for searching. Shrunk copies are made on demand
/// and reused across seeds.
pub struct Searcher {
    full: Gray,
    full_ii: Integral,
    shrunk: HashMap<usize, (Gray, Integral)>,
}

impl Searcher {
    pub fn new(img: Gray) -> Searcher {
        let full_ii = Integral::new(&img);
        Searcher {
            full: img,
            full_ii,
            shrunk: HashMap::new(),
        }
    }

    /// Best match for the template anywhere in the image (position in full-size pixels).
    pub fn find(&mut self, t: &Template) -> Option<Match> {
        let Searcher {
            full,
            full_ii,
            shrunk,
        } = self;
        if t.w > full.w || t.h > full.h {
            return None;
        }
        let f = t.factor;
        if f == 1 {
            return scan_best(full, full_ii, &t.full, 1).into_iter().next();
        }

        let (small, small_ii) = shrunk.entry(f).or_insert_with(|| {
            let g = full.downscale(f);
            let ii = Integral::new(&g);
            (g, ii)
        });
        if t.coarse.w > small.w || t.coarse.h > small.h {
            return None;
        }

        // Check the few best coarse spots again at full size, nudging around each.
        let mut best: Option<Match> = None;
        for cand in scan_best(small, small_ii, &t.coarse, 5) {
            let (cx, cy) = (cand.x * f, cand.y * f);
            let reach = 2 * f;
            let (x_hi, y_hi) = ((cx + reach).min(full.w - t.w), (cy + reach).min(full.h - t.h));
            for y in cy.saturating_sub(reach)..=y_hi {
                for x in cx.saturating_sub(reach)..=x_hi {
                    let score = zncc_at(full, full_ii, &t.full, x, y);
                    if best.map_or(true, |b| score > b.score) {
                        best = Some(Match { score, x, y });
                    }
                }
            }
        }
        best
    }
}

/// Scans every position and returns up to `keep` high-scoring spots that
/// aren't on top of each other, best first.
fn scan_best(img: &Gray, ii: &Integral, t: &Level, keep: usize) -> Vec<Match> {
    let mut all = Vec::with_capacity((img.w - t.w + 1) * (img.h - t.h + 1));
    for y in 0..=img.h - t.h {
        for x in 0..=img.w - t.w {
            all.push(Match {
                score: zncc_at(img, ii, t, x, y),
                x,
                y,
            });
        }
    }
    all.sort_by(|a, b| b.score.total_cmp(&a.score));

    let gap = (t.w.min(t.h) / 2).max(1);
    let mut picked: Vec<Match> = Vec::with_capacity(keep);
    for m in all {
        if picked.len() >= keep {
            break;
        }
        if picked
            .iter()
            .all(|p| p.x.abs_diff(m.x) > gap || p.y.abs_diff(m.y) > gap)
        {
            picked.push(m);
        }
    }
    picked
}

