//! Turning the text read off the shop into a status per seed.
//!
//! The shop lists every seed all the time. Under each name is either
//! "Out of Stock" or "Stock: N". A seed only counts as in stock when its name
//! is read AND the line under it reads "Stock: N", so a row cut off at the
//! bottom of the list, or a misread line, never causes an alert.
//!
//! The right half of the shop shows the selected seed: its name at the top
//! and, further down, "Stock: N" above the buy buttons, or a gray
//! "Out of Stock" button. It stays selected across restocks, so it lets one
//! more seed be watched than fit in the list.

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
    /// Read from the selected-seed panel on the right, not the list.
    pub in_panel: bool,
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

/// Reads the shop. `width` is the game window's width: the list is in its
/// left half, the selected seed in its right half. `known` is every seed name
/// we know of (so one seed's name isn't mistaken for a similar one); `wanted`
/// are the seeds to report on.
pub fn read_shop(lines: &[Line], width: f32, known: &[String], wanted: &[String]) -> ShopView {
    let mut all: Vec<(&str, String)> = known.iter().chain(wanted).map(|n| (n.as_str(), normalize(n))).collect();
    all.sort_by(|a, b| a.0.cmp(b.0));
    all.dedup_by(|a, b| a.0 == b.0);

    let mid = width / 2.0;
    let names: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| which_seed(&l.text, &all).map(|n| (i, n.to_string())))
        .collect();

    let shop_open = lines.iter().any(|l| {
        let n = normalize(&l.text);
        classify(&l.text).is_some() || n.contains("untilnextdeal") || n.contains("restock")
    });

    let panel = read_panel(lines, mid, &names);
    let seeds = wanted
        .iter()
        .map(|want| {
            let row = read_list_row(lines, mid, &names, want);
            match &panel {
                // The list wins, unless it couldn't tell and the panel can.
                Some(p)
                    if p.name == *want
                        && (row.status == Status::NotVisible
                            || (row.status == Status::Unknown && p.status != Status::Unknown)) =>
                {
                    SeedView {
                        name: want.clone(),
                        status: p.status,
                        name_line: p.name_line,
                        stock_line: p.stock_line,
                        in_panel: true,
                    }
                }
                _ => row,
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

/// A seed's row in the list (left of `mid`): its name and the line under it.
fn read_list_row(lines: &[Line], mid: f32, names: &[(usize, String)], want: &str) -> SeedView {
    let Some(&(ni, _)) = names.iter().find(|(i, n)| n == want && lines[*i].x < mid) else {
        return SeedView {
            name: want.to_string(),
            status: Status::NotVisible,
            name_line: None,
            stock_line: None,
            in_panel: false,
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
    SeedView {
        name: want.to_string(),
        status: below.and_then(|i| classify(&lines[i].text)).unwrap_or(Status::Unknown),
        name_line: Some(ni),
        stock_line: below,
        in_panel: false,
    }
}

/// The selected seed, right of `mid`: the topmost seed name there, and the
/// nearest "Stock: N" or "Out of Stock" (the gray buy button) below it. The
/// description, prices and "Send Gift" in between match neither.
fn read_panel(lines: &[Line], mid: f32, names: &[(usize, String)]) -> Option<SeedView> {
    let (ti, name) = names
        .iter()
        .filter(|(i, _)| lines[*i].x >= mid)
        .min_by(|a, b| lines[a.0].y.total_cmp(&lines[b.0].y))?;
    let title = &lines[*ti];
    let stock = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| i != ti && l.x >= mid && l.y > title.y + 0.5 * title.h)
        .filter_map(|(i, l)| classify(&l.text).map(|s| (i, l.y, s)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    Some(SeedView {
        name: name.clone(),
        status: stock.map_or(Status::Unknown, |(_, _, s)| s),
        name_line: Some(*ti),
        stock_line: stock.map(|(i, _, _)| i),
        in_panel: true,
    })
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
        read_shop(lines, 1920.0, &known(), &wanted).seeds.iter().map(|s| s.status).collect()
    }

    #[test]
    fn reads_real_shop() {
        let view = read_shop(&shop(), 1920.0, &known(), &known());
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

    #[test]
    fn tolerates_ocr_mistakes() {
        let lines = vec![
            line("Baobab Tree Sced", 527.0, 274.0, 30.0),
            line("0ut of Stock", 527.0, 317.0, 22.0),
            line("Maple Tree Seed", 527.0, 407.0, 30.0),
            line("Stock; l", 527.0, 450.0, 22.0),
            line("Star Fruit Seed", 527.0, 672.0, 30.0),
            line("Stock:10", 527.0, 715.0, 22.0),
        ];
        assert_eq!(
            statuses(&lines, &["Baobab Tree Seed", "Maple Tree Seed", "Star Fruit Seed"]),
            vec![Status::OutOfStock, Status::InStock(1), Status::InStock(10)]
        );
    }

    #[test]
    fn similar_names_are_not_confused() {
        let mut names = known();
        names.push("Bear Seed".into());
        let lines = vec![line("Bean Seed", 527.0, 539.0, 30.0), line("Stock: 1", 527.0, 582.0, 22.0)];
        let wanted = vec!["Bear Seed".to_string()];
        let view = read_shop(&lines, 1920.0, &names, &wanted);
        assert_eq!(view.seeds[0].status, Status::NotVisible);
    }

    #[test]
    fn longer_names_do_not_count_as_shorter_ones() {
        let known = crate::seedlist::built_in().seeds;
        let lines = vec![
            line("Sweet Potato Seed", 527.0, 274.0, 30.0),
            line("Stock: 1", 527.0, 317.0, 22.0),
            line("Sugar Apple Seed", 527.0, 407.0, 30.0),
            line("Stock: 1", 527.0, 450.0, 22.0),
        ];
        let wanted: Vec<String> = ["Potato Seed", "Apple Seed", "Sweet Potato Seed", "Sugar Apple Seed"]
            .map(String::from)
            .to_vec();
        let got: Vec<Status> = read_shop(&lines, 1920.0, &known, &wanted).seeds.iter().map(|s| s.status).collect();
        use Status::*;
        assert_eq!(got, vec![NotVisible, NotVisible, InStock(1), InStock(1)]);
    }

    #[test]
    fn shop_closed() {
        let lines = vec![line("Inventory", 100.0, 100.0, 30.0)];
        let view = read_shop(&lines, 1920.0, &known(), &["Baobab Tree Seed".to_string()]);
        assert!(!view.shop_open);
        assert_eq!(view.seeds[0].status, Status::NotVisible);
    }

    /// The shop with Jacaranda selected (1336 wide), as in a real screenshot:
    /// the list on the left, Jacaranda's panel on the right with `stock`.
    fn shop_with_panel(stock: Option<(&str, f32, f32)>) -> Vec<Line> {
        let mut lines = vec![
            line("Until next deal: 1min 56sec", 362.0, 85.0, 20.0),
            line("53.1M", 780.0, 82.0, 22.0),
            line("Watermelon Seed", 250.0, 162.0, 30.0),
            line("Out of Stock", 250.0, 205.0, 20.0),
            line("Mango Seed", 250.0, 427.0, 30.0),
            line("Out of Stock", 250.0, 470.0, 20.0),
            line("Jacaranda Seed", 822.0, 175.0, 35.0),
            line("Can be harvested once.", 766.0, 237.0, 24.0),
            line("Send Gift", 1165.0, 238.0, 18.0),
            line("46", 1120.0, 690.0, 30.0),
        ];
        if let Some((text, x, y)) = stock {
            lines.push(line(text, x, y, 26.0));
        }
        lines
    }

    fn panel_view(lines: &[Line], wanted: &[&str]) -> Vec<(Status, bool)> {
        let names: Vec<String> = ["Watermelon Seed", "Mango Seed", "Jacaranda Seed"].map(String::from).to_vec();
        let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
        read_shop(lines, 1336.0, &names, &wanted).seeds.iter().map(|s| (s.status, s.in_panel)).collect()
    }

    #[test]
    fn panel_seed_in_stock() {
        let lines = shop_with_panel(Some(("Stock: 1", 834.0, 628.0)));
        assert_eq!(
            panel_view(&lines, &["Jacaranda Seed", "Mango Seed"]),
            vec![(Status::InStock(1), true), (Status::OutOfStock, false)]
        );
    }

    #[test]
    fn panel_seed_out_of_stock_button() {
        // No "Stock:" line; the coin button turns gray and says "Out of Stock".
        let lines = shop_with_panel(Some(("Out of Stock", 790.0, 696.0)));
        assert_eq!(panel_view(&lines, &["Jacaranda Seed"]), vec![(Status::OutOfStock, true)]);
    }

    #[test]
    fn panel_without_stock_line_is_unknown() {
        assert_eq!(panel_view(&shop_with_panel(None), &["Jacaranda Seed"]), vec![(Status::Unknown, true)]);
    }

    #[test]
    fn panel_seed_not_wanted_is_ignored() {
        let lines = shop_with_panel(Some(("Stock: 1", 834.0, 628.0)));
        assert_eq!(panel_view(&lines, &["Watermelon Seed"]), vec![(Status::OutOfStock, false)]);
    }

    #[test]
    fn panel_title_is_not_a_list_row() {
        // Only the panel's name, no "Stock" near it: must not borrow the
        // description line or anything from the list.
        let lines = vec![
            line("Jacaranda Seed", 822.0, 175.0, 35.0),
            line("Can be harvested once.", 766.0, 237.0, 24.0),
            line("Mango Seed", 250.0, 160.0, 30.0),
            line("Stock: 3", 250.0, 203.0, 20.0),
        ];
        assert_eq!(panel_view(&lines, &["Jacaranda Seed"]), vec![(Status::Unknown, true)]);
    }

    #[test]
    fn list_wins_when_seed_is_in_both() {
        let mut lines = shop_with_panel(None);
        lines.extend([line("Jacaranda Seed", 250.0, 560.0, 30.0), line("Stock: 2", 250.0, 603.0, 20.0)]);
        assert_eq!(panel_view(&lines, &["Jacaranda Seed"]), vec![(Status::InStock(2), false)]);
    }

    /// Guards seeds.json updates: every listed name must be recognised as
    /// itself, and no two names may be close enough to be mixed up.
    #[test]
    fn seed_list_names_are_distinct() {
        let list = crate::seedlist::built_in().seeds;
        let all: Vec<(&str, String)> = list.iter().map(|n| (n.as_str(), normalize(n))).collect();
        for name in &list {
            assert_eq!(which_seed(name, &all), Some(name.as_str()), "{name} isn't recognised as itself");
        }
        for (i, (a, na)) in all.iter().enumerate() {
            for (b, nb) in &all[i + 1..] {
                let d = edit_distance(na, nb);
                assert!(
                    d > tolerance(na).max(tolerance(nb)),
                    "\"{a}\" and \"{b}\" are too alike to tell apart reliably"
                );
            }
        }
    }

    #[test]
    fn classify_lines() {
        assert_eq!(classify("Out of Stock"), Some(Status::OutOfStock));
        assert_eq!(classify("Stock: 3"), Some(Status::InStock(3)));
        assert_eq!(classify("Stock: 12"), Some(Status::InStock(12)));
        assert_eq!(classify("Can be harvested once."), None);
        assert_eq!(classify("Stock"), None);
    }
}
