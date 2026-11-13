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

