//! Turning the text read off the shop into a status per seed.
//!
//! The shop lists every seed all the time. Under each name is either
//! "Out of Stock" or "Stock: N". A seed only counts as in stock when its name
//! is read AND the line under it reads "Stock: N", so a row cut off at the
//! bottom of the list, or a misread line, never causes an alert.

use crate::ocr::Line;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// "Stock: N" under the name.
    InStock(u32),
    /// "Out of Stock" under the name.
    OutOfStock,
    /// Name read, but not the line under it (cut off, or misread).
    Unknown,
    /// Name not read (scrolled out of view, or the shop is closed).
    NotVisible,
}

impl Status {
    pub fn describe(self) -> String {
        match self {
            Status::InStock(n) => format!("IN STOCK ({n} left)"),
            Status::OutOfStock => "Out of stock".into(),
            Status::Unknown => "Can't read its stock line".into(),
            Status::NotVisible => "Not on screen".into(),
        }
    }
}

pub struct SeedView {
    pub name: String,
    pub status: Status,
    /// Index into the lines of the seed's name and of the line under it.
    pub name_line: Option<usize>,
    pub stock_line: Option<usize>,
}

pub struct ShopView {
    /// True if anything that only appears in the seed shop was read.
    pub shop_open: bool,
    /// Time until the shop's next restock, e.g. "1min 37sec".
    pub next_deal: Option<String>,
    /// One per wanted seed, in the same order.
    pub seeds: Vec<SeedView>,
    /// Every line recognised as some seed's name: (line index, seed name).
    pub names: Vec<(usize, String)>,
}

/// Lowercase letters and digits only: "Stock: 1" -> "stock1".
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// How many misread letters a name may have and still count.
fn tolerance(norm_name: &str) -> usize {
    match norm_name.chars().count() {
        0..=6 => 0,
        7..=11 => 1,
        _ => 2,
    }
}

/// Which seed a line of text is the name of, if any. When two seeds are
/// equally close, it's too unclear to say.
fn which_seed<'a>(text: &str, seeds: &[(&'a str, String)]) -> Option<&'a str> {
    let line = normalize(text);
    if line.is_empty() {
        return None;
    }
    let mut best: Option<(usize, &str)> = None;
    let mut tie = false;
    for (name, norm) in seeds {
        let d = if line.contains(norm.as_str()) && line.len() <= norm.len() + 3 {
            0 // a stray mark read before or after the name
        } else {
            edit_distance(&line, norm)
        };
        if d > tolerance(norm) {
            continue;
        }
        match best {
            Some((bd, _)) if d > bd => {}
            Some((bd, bn)) if d == bd => tie |= bn != *name,
            _ => {
                best = Some((d, name));
                tie = false;
            }
        }
    }
    if tie {
        None
    } else {
        best.map(|(_, n)| n)
    }
}

/// What a stock line says: "Out of Stock" or "Stock: N".
pub fn classify(text: &str) -> Option<Status> {
    let s = normalize(text);
    if s.contains("outof") || edit_distance(&s, "outofstock") <= 2 {
        return Some(Status::OutOfStock);
    }
    let start: String = s.chars().take(5).collect();
    if s.chars().count() > 5 && edit_distance(&start, "stock") <= 1 {
        // Letters OCR commonly reads in place of digits.
        let digits: String = s
            .chars()
            .skip(5)
            .map(|c| match c {
                'o' => '0',
                'i' | 'l' => '1',
                c => c,
            })
            .collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(Status::InStock(n));
        }
    }
    None
}

/// "Until next deal: Imin 37sec" -> "1min 37sec".
fn next_deal(text: &str) -> Option<String> {
    if !normalize(text).contains("untilnextdeal") {
        return None;
    }
    let after = text.split_once(':').map_or("", |(_, rest)| rest).trim();
    let fixed: Vec<String> = after
        .split_whitespace()
        .map(|word| {
            let unit_at = word.find(|c: char| c.is_ascii_lowercase() && !matches!(c, 'l' | 'o')).unwrap_or(word.len());
            let (num, unit) = word.split_at(unit_at);
            let num: String = num
                .chars()
                .map(|c| match c {
                    'I' | 'l' | '|' => '1',
                    'O' | 'o' => '0',
                    c => c,
                })
                .collect();
            format!("{num}{unit}")
        })
        .collect();
    (!fixed.is_empty()).then(|| fixed.join(" "))
}

/// Reads the shop. `known` is every seed name we know of (so one seed's name
/// isn't mistaken for a similar one); `wanted` are the seeds to report on.
pub fn read_shop(lines: &[Line], known: &[String], wanted: &[String]) -> ShopView {
    let mut all: Vec<(&str, String)> = known.iter().chain(wanted).map(|n| (n.as_str(), normalize(n))).collect();
    all.sort_by(|a, b| a.0.cmp(b.0));
    all.dedup_by(|a, b| a.0 == b.0);

    let names: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| which_seed(&l.text, &all).map(|n| (i, n.to_string())))
        .collect();

    let shop_open = lines.iter().any(|l| {
        let n = normalize(&l.text);
        classify(&l.text).is_some() || n.contains("untilnextdeal") || n.contains("restock")
    });

    let seeds = wanted
        .iter()
        .map(|want| {
            let Some(&(ni, _)) = names.iter().find(|(_, n)| n == want) else {
                return SeedView {
                    name: want.clone(),
                    status: Status::NotVisible,
                    name_line: None,
                    stock_line: None,
                };
            };
            let name = &lines[ni];
            // The line just under the name, starting about where it starts.
            let below = lines
                .iter()
                .enumerate()
                .filter(|(i, l)| {
                    *i != ni
                        && l.y > name.y + 0.5 * name.h
                        && l.y < name.y + 2.5 * name.h
                        && (l.x - name.x).abs() < 1.5 * name.h
                })
                .min_by(|a, b| a.1.y.total_cmp(&b.1.y))
                .map(|(i, _)| i);
            let status = below.and_then(|i| classify(&lines[i].text)).unwrap_or(Status::Unknown);
            SeedView {
                name: want.clone(),
                status,
                name_line: Some(ni),
                stock_line: below,
            }
        })
        .collect();

    ShopView {
        shop_open,
        next_deal: lines.iter().find_map(|l| next_deal(&l.text)),
        seeds,
        names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f32, y: f32, h: f32) -> Line {
        Line {
            text: text.into(),
            x,
            y,
            w: 200.0,
            h,
        }
    }

    /// What the game's shop actually read as at 1920x1080.
    fn shop() -> Vec<Line> {
        vec![
            line("Until next deal: Imin 37sec", 639.0, 204.0, 22.0),
            line("Baobab Tree Seed", 527.0, 274.0, 30.0),
            line("Out of Stock", 527.0, 317.0, 22.0),
            line("Maple Tree Seed", 527.0, 407.0, 30.0),
            line("Out of Stock", 527.0, 450.0, 22.0),
            line("Bean Seed", 527.0, 539.0, 30.0),
            line("Out of Stock", 527.0, 582.0, 22.0),
            line("Star Fruit Seed", 527.0, 672.0, 30.0),
            line("Out of Stock", 527.0, 715.0, 22.0),
            line("Sugar Apple Seed", 526.0, 804.0, 30.0),
            line("Stock: 1", 527.0, 846.0, 22.0),
        ]
    }

    fn known() -> Vec<String> {
        ["Baobab Tree Seed", "Maple Tree Seed", "Bean Seed", "Star Fruit Seed", "Sugar Apple Seed"]
            .map(String::from)
            .to_vec()
    }

    fn statuses(lines: &[Line], wanted: &[&str]) -> Vec<Status> {
        let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
        read_shop(lines, &known(), &wanted).seeds.iter().map(|s| s.status).collect()
    }

    #[test]
    fn reads_real_shop() {
        let view = read_shop(&shop(), &known(), &known());
        let got: Vec<Status> = view.seeds.iter().map(|s| s.status).collect();
        use Status::*;
        assert_eq!(got, vec![OutOfStock, OutOfStock, OutOfStock, OutOfStock, InStock(1)]);
        assert!(view.shop_open);
        assert_eq!(view.next_deal.as_deref(), Some("1min 37sec"));
    }

    #[test]
    fn seed_not_in_view() {
        let lines: Vec<Line> = shop().into_iter().skip(3).collect(); // Baobab scrolled away
        assert_eq!(statuses(&lines, &["Baobab Tree Seed"]), vec![Status::NotVisible]);
    }

    #[test]
    fn last_row_cut_off_is_unknown_not_in_stock() {
        let mut lines = shop();
        lines.pop(); // "Stock: 1" below the edge of the list
        assert_eq!(statuses(&lines, &["Sugar Apple Seed"]), vec![Status::Unknown]);
    }

    #[test]
    fn next_rows_stock_line_does_not_count() {
        let lines = vec![
            line("Baobab Tree Seed", 527.0, 274.0, 30.0),
            // its own stock line wasn't read; the next row's is far below
            line("Maple Tree Seed", 527.0, 407.0, 30.0),
            line("Stock: 2", 527.0, 450.0, 22.0),
        ];
        assert_eq!(statuses(&lines, &["Baobab Tree Seed"]), vec![Status::Unknown]);
    }
}
