//! The search surface: the library as a filterable table, after the Figma
//! mock's Search screen. Headless like the page: the host turns keys into
//! `SearchInput`, applies the `SearchAction` it gets back (a rating or tag
//! goes to the sidecar store, an open becomes a setlist), and draws the
//! `Frame` that `frame` composes.
//!
//! Matching is chordpro-tui's finder (`internal/tui/picker.go`): a
//! case-insensitive subsequence over title and artist, scored for word
//! starts and runs, an artist hit ranked just under a title hit. The query
//! can also carry filters, which become pills: `+tag ` filters on a tag,
//! `*3` keeps songs rated three stars or more, and `l:` searches lyrics too.

use crate::{Config, Face, Frame, Ink, Measure, Polarity, Shape, Text};
use crate::{library::Library, store::Store};

/// What the rows are ordered by when there is no query to rank them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Title,
    Artist,
    Rating,
    /// Most recently shown first; songs never shown last, by title.
    Recent,
}

impl SortBy {
    fn next(self) -> Self {
        match self {
            Self::Title => Self::Artist,
            Self::Artist => Self::Rating,
            Self::Rating => Self::Recent,
            Self::Recent => Self::Title,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Title => "TITLE",
            Self::Artist => "ARTIST",
            Self::Rating => "RATING",
            Self::Recent => "RECENT",
        }
    }
}

/// A key, already decoded by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchInput {
    Char(char),
    Backspace,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Tab: the next sort order.
    Sort,
    Enter,
    /// Escape. Ends tag entry; otherwise the host closes search.
    Cancel,
}

/// What the host must do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchAction {
    /// Redraw; nothing outside search changed.
    Redraw,
    /// Rate the selected song (0 clears).
    Rate { entry: usize, rating: u8 },
    /// Toggle a tag on the selected song.
    Tag { entry: usize, tag: String },
    /// Open the selected song with the visible results as the setlist.
    Open {
        entries: Vec<usize>,
        position: usize,
    },
    /// Leave search for the page.
    Close,
}

#[derive(Debug, Clone, PartialEq)]
struct Match {
    entry: usize,
    title: Vec<usize>,
    artist: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Search {
    /// The free-text part of the query.
    pub query: String,
    pub tags: Vec<String>,
    pub min_rating: u8,
    pub sort: SortBy,
    /// `#` began entering a tag for the selected song.
    pub tag_entry: Option<String>,
    /// A one-line note for the footer, such as what a key just did.
    pub notice: Option<String>,
    cursor: usize,
    top: usize,
    matches: Vec<Match>,
}

impl Search {
    pub fn new(library: &Library, store: &Store) -> Self {
        let mut search = Self::default();
        search.rematch(library, store);
        search
    }

    /// The library index of the selected row.
    pub fn selected(&self) -> Option<usize> {
        self.matches.get(self.cursor).map(|m| m.entry)
    }

    /// The library indices of the rows, in order.
    pub fn results(&self) -> Vec<usize> {
        self.matches.iter().map(|m| m.entry).collect()
    }

    /// Puts the cursor on library entry `entry` if it is listed.
    pub fn select(&mut self, entry: usize) {
        if let Some(row) = self.matches.iter().position(|m| m.entry == entry) {
            self.cursor = row;
        }
    }

    pub fn input(&mut self, input: SearchInput, library: &Library, store: &Store) -> SearchAction {
        self.notice = None;
        if let Some(tag) = &mut self.tag_entry {
            match input {
                SearchInput::Char(c) if !c.is_control() && c != ' ' => tag.push(c),
                SearchInput::Backspace => {
                    if tag.pop().is_none() {
                        self.tag_entry = None;
                    }
                }
                SearchInput::Cancel => self.tag_entry = None,
                SearchInput::Enter => {
                    let tag = self.tag_entry.take().unwrap_or_default();
                    let tag = crate::library::normalise_tag(&tag);
                    if let Some(entry) = self.selected().filter(|_| !tag.is_empty()) {
                        return SearchAction::Tag { entry, tag };
                    }
                }
                _ => {}
            }
            return SearchAction::Redraw;
        }
        match input {
            SearchInput::Cancel => return SearchAction::Close,
            SearchInput::Enter => {
                if !self.matches.is_empty() {
                    return SearchAction::Open {
                        entries: self.results(),
                        position: self.cursor,
                    };
                }
            }
            SearchInput::Up => self.move_by(-1),
            SearchInput::Down => self.move_by(1),
            SearchInput::PageUp => self.move_by(-10),
            SearchInput::PageDown => self.move_by(10),
            SearchInput::Home => self.cursor = 0,
            SearchInput::End => self.cursor = self.matches.len().saturating_sub(1),
            SearchInput::Sort => {
                self.sort = self.sort.next();
                self.refilter(library, store);
            }
            SearchInput::Backspace => {
                if self.query.pop().is_none() {
                    // An empty query backs into the pills, newest first.
                    if self.tags.pop().is_none() {
                        self.min_rating = 0;
                    }
                }
                self.requery(library, store);
            }
            SearchInput::Char('#') => self.tag_entry = Some(String::new()),
            SearchInput::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if self.query.ends_with('*') {
                    // `*3` keeps three stars and up.
                    self.query.pop();
                    self.min_rating = digit.min(5);
                    self.requery(library, store);
                } else if digit <= 5
                    && let Some(entry) = self.selected()
                {
                    return SearchAction::Rate {
                        entry,
                        rating: digit,
                    };
                }
            }
            SearchInput::Char(' ') if last_word(&self.query).starts_with('+') => {
                let word = last_word(&self.query).to_owned();
                self.query.truncate(self.query.len() - word.len());
                let tag = crate::library::normalise_tag(&word[1..]);
                if !tag.is_empty() && !self.tags.contains(&tag) {
                    self.tags.push(tag);
                }
                self.requery(library, store);
            }
            SearchInput::Char(c) if !(c.is_control() || c == ' ' && self.query.is_empty()) => {
                self.query.push(c);
                self.requery(library, store);
            }
            _ => {}
        }
        SearchAction::Redraw
    }

    fn move_by(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }

    /// The text the rows are matched against: the query without filter
    /// words still being typed, and whether lyrics are included.
    fn terms(&self) -> (String, bool) {
        let mut query = self.query.trim().to_owned();
        let lyrics = query.starts_with("l:");
        if lyrics {
            query = query[2..].trim().to_owned();
        }
        let words: Vec<&str> = query
            .split(' ')
            .filter(|w| !w.starts_with('+') && !w.starts_with('*'))
            .collect();
        (words.join(" ").trim().to_owned(), lyrics)
    }

    /// Recomputes the rows after the sort, store or library changed,
    /// keeping the selected song selected when it is still listed.
    pub fn refilter(&mut self, library: &Library, store: &Store) {
        let selected = self.selected();
        self.rematch(library, store);
        if let Some(entry) = selected {
            self.select(entry);
        }
    }

    /// Recomputes the rows after a rescan replaced `old` with `new`. Entry
    /// indices move when files come and go, so the selection follows the
    /// selected song's path; if that song is gone the best match is selected.
    pub fn rescanned(&mut self, old: &Library, new: &Library, store: &Store) {
        let path = self.selected().map(|entry| &old.entries[entry].path);
        let entry = path.and_then(|path| new.entries.iter().position(|e| &e.path == path));
        self.rematch(new, store);
        match entry {
            Some(entry) => self.select(entry),
            None => self.top = 0,
        }
    }

    /// Recomputes the rows after the query or a filter changed, with the
    /// best match selected, as chordpro-tui's finder does.
    fn requery(&mut self, library: &Library, store: &Store) {
        self.rematch(library, store);
        self.top = 0;
    }

    fn rematch(&mut self, library: &Library, store: &Store) {
        let (query, lyrics) = self.terms();
        let mut hits: Vec<(i32, Match)> = Vec::new();
        for (index, entry) in library.entries.iter().enumerate() {
            let rating = store.rating(&entry.fingerprint);
            if rating < self.min_rating {
                continue;
            }
            let has = |tag: &String| {
                entry.file_tags.contains(tag) || store.tags(&entry.fingerprint).contains(tag)
            };
            if !self.tags.iter().all(has) {
                continue;
            }
            let mut found = Match {
                entry: index,
                title: Vec::new(),
                artist: Vec::new(),
            };
            if query.is_empty() {
                hits.push((0, found));
                continue;
            }
            let mut best = None;
            if let Some((score, positions)) = fuzzy_match(&query, &entry.title) {
                best = Some(score);
                found.title = positions;
            }
            if !entry.artist.is_empty()
                && let Some((score, positions)) = fuzzy_match(&query, &entry.artist)
            {
                const ARTIST_BIAS: i32 = 2;
                found.artist = positions;
                best = Some(best.map_or(score - ARTIST_BIAS, |b: i32| b.max(score - ARTIST_BIAS)));
            }
            if best.is_none() && lyrics && entry.lyrics.contains(&query.to_lowercase()) {
                // A lyric hit ranks under any title or artist hit.
                best = Some(-1000);
            }
            if let Some(score) = best {
                hits.push((score, found));
            }
        }
        let entries = &library.entries;
        let key = |m: &Match| -> (u64, String) {
            let entry = &entries[m.entry];
            match self.sort {
                SortBy::Title => (0, entry.title.to_lowercase()),
                SortBy::Artist => (
                    0,
                    entry.artist.to_lowercase() + "\u{0}" + &entry.title.to_lowercase(),
                ),
                SortBy::Rating => (
                    u64::from(5 - store.rating(&entry.fingerprint)),
                    entry.title.to_lowercase(),
                ),
                SortBy::Recent => (
                    u64::MAX - store.opened(&entry.fingerprint),
                    entry.title.to_lowercase(),
                ),
            }
        };
        hits.sort_by(|(sa, a), (sb, b)| sb.cmp(sa).then_with(|| key(a).cmp(&key(b))));
        self.matches = hits.into_iter().map(|(_, m)| m).collect();
        self.cursor = 0;
    }

    /// The search screen at `width`×`height` points. Measures text, so it
    /// runs where the page is composed, on the host's worker.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        width: f32,
        height: f32,
        polarity: Polarity,
        library: &Library,
        store: &Store,
        config: &Config,
        measure: &impl Measure,
    ) -> Frame {
        let c = config;
        let mut frame = Frame {
            width,
            height,
            polarity,
            text: Vec::new(),
            shapes: Vec::new(),
            fits: true,
            columns: 1,
            ladder_step: 0,
        };
        let left = c.margin_x;
        let right = width - c.margin_x;
        let text = |content: &str, x: f32, baseline: f32, size: f32, weight: u16, ink: Ink| Text {
            content: content.to_owned(),
            group: 0,
            x,
            baseline,
            size,
            weight,
            face: Face::System,
            ink,
        };
        let w = |content: &str, size: f32, weight: u16| {
            measure.width(content, size, weight, Face::System)
        };

        // The query line: a slash, the query (or the tag being entered), a
        // caret, and a hint; a hairline under it.
        let q = c.search_query_size;
        let baseline = c.margin_top + q;
        let (prompt, typed) = match &self.tag_entry {
            Some(tag) => ("#", tag.as_str()),
            None => ("/", self.query.as_str()),
        };
        frame
            .text
            .push(text(prompt, left, baseline, q, 300, Ink::Faint));
        let query_x = left + w(prompt, q, 300) + q * 0.5;
        frame
            .text
            .push(text(typed, query_x, baseline, q, 400, Ink::Strong));
        let caret_x = query_x + w(typed, q, 400) + q * 0.15;
        frame.shapes.push(Shape {
            group: 0,
            x: caret_x,
            y: baseline - q * 0.95,
            width: (q * 0.08).max(1.0),
            height: q * 1.15,
            radius: 0.0,
            stroke: None,
            ink: Ink::Strong,
        });
        let hint = match (&self.tag_entry, self.selected()) {
            (Some(_), Some(entry)) => format!(
                "tag for {}  ·  ⏎ adds or removes  ·  esc cancels",
                library.entries[entry].title
            ),
            (Some(_), None) => "no song selected".to_owned(),
            (None, _) => "title · artist   (l: lyrics, +tag filters, *3 rating)".to_owned(),
        };
        let hint_x = caret_x + q * 0.6;
        let hint_size = c.search_label_size;
        frame.text.push(text(
            &fit(&hint, right - hint_x, hint_size, 400, &w),
            hint_x,
            baseline,
            hint_size,
            400,
            Ink::Faint,
        ));
        let rule_y = baseline + q * 0.95;
        frame
            .shapes
            .push(rule(left, rule_y, right - left, Ink::Faint));

        // Filters: a pill per tag filter and the rating floor, filled, then
        // the hint pills for adding a tag and the sort order, outlined.
        let pill_h = c.search_pill_height;
        let centre = rule_y + pill_h * 1.3;
        let label = c.search_label_size;
        let label_baseline = centre + label * 0.35;
        frame.text.push(text(
            "FILTERS",
            left,
            label_baseline,
            label,
            600,
            Ink::Faint,
        ));
        let mut x = left + w("FILTERS", label, 600) + c.search_column_gap * 4.0;
        let mut pills: Vec<(String, bool)> = self
            .tags
            .iter()
            .map(|tag| (format!("#{tag}  ×"), true))
            .collect();
        if self.min_rating > 0 {
            pills.push((format!("★ {}+  ×", self.min_rating), true));
        }
        pills.push(("+ tag".to_owned(), false));
        pills.push((format!("SORT  {}", self.sort.label()), false));
        for (content, filled) in pills {
            let pill_w = w(&content, label, 700) + 2.0 * c.search_pill_pad;
            if x + pill_w > right {
                break;
            }
            frame.shapes.push(Shape {
                group: 0,
                x,
                y: centre - pill_h / 2.0,
                width: pill_w,
                height: pill_h,
                radius: pill_h / 2.0,
                stroke: (!filled).then_some(c.search_box_stroke * 0.5),
                ink: if filled { Ink::Strong } else { Ink::Faint },
            });
            frame.text.push(text(
                &content,
                x + c.search_pill_pad,
                label_baseline,
                label,
                700,
                if filled { Ink::Paper } else { Ink::Soft },
            ));
            x += pill_w + c.search_pill_pad * 0.8;
        }

        // The results box, its header row and as many rows as fit.
        let box_top = centre + pill_h * 1.4;
        let box_bottom = height - c.margin_bottom - c.status_height * c.body_size;
        let stroke = c.search_box_stroke;
        frame.shapes.push(Shape {
            group: 0,
            x: left,
            y: box_top,
            width: right - left,
            height: (box_bottom - box_top).max(0.0),
            radius: c.search_box_radius,
            stroke: Some(stroke),
            ink: Ink::Soft,
        });
        let size = c.search_text_size;
        let inner_left = left + stroke + c.search_column_gap;
        let inner_right = right - stroke - c.search_column_gap;
        let columns = Columns::new(
            inner_right - inner_left,
            c.search_column_gap,
            c.search_min_title,
            library,
            store,
            &self.matches,
            |s, weight| w(s, size, weight),
            |s| w(s, label, 600),
        );
        let header_top = box_top + stroke;
        let header_baseline = header_top + c.search_header_row * 0.5 + label * 0.35;
        for (name, x, _) in columns.visible(inner_left) {
            frame
                .text
                .push(text(name, x, header_baseline, label, 600, Ink::Soft));
        }
        let rows_top = header_top + c.search_header_row;
        let row_h = c.search_row;
        let room = ((box_bottom - stroke - rows_top) / row_h).floor().max(0.0) as usize;
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if room > 0 && self.cursor >= self.top + room {
            self.top = self.cursor + 1 - room;
        }
        self.top = self.top.min(self.matches.len().saturating_sub(room.max(1)));
        for (row, m) in self.matches.iter().enumerate().skip(self.top).take(room) {
            let top = rows_top + (row - self.top) as f32 * row_h;
            if row == self.cursor {
                frame.shapes.push(Shape {
                    group: 0,
                    x: left + stroke,
                    y: top,
                    width: right - left - 2.0 * stroke,
                    height: row_h,
                    radius: 0.0,
                    stroke: None,
                    ink: Ink::Wash,
                });
            }
            let baseline = top + row_h * 0.5 + size * 0.35;
            let entry = &library.entries[m.entry];
            for (name, x, width) in columns.visible(inner_left) {
                let width = width - c.search_column_gap;
                match name {
                    "TITLE" => highlighted(
                        &mut frame,
                        &entry.title,
                        &m.title,
                        x,
                        width,
                        baseline,
                        size,
                        Ink::Strong,
                        &w,
                    ),
                    "ARTIST" => highlighted(
                        &mut frame,
                        &entry.artist,
                        &m.artist,
                        x,
                        width,
                        baseline,
                        size,
                        Ink::Soft,
                        &w,
                    ),
                    _ => {
                        let (content, ink) = cell(name, entry, store);
                        frame.text.push(text(
                            &fit(&content, width, size, 400, &w),
                            x,
                            baseline,
                            if name == "TAGS" { label + 1.0 } else { size },
                            400,
                            ink,
                        ));
                    }
                }
            }
        }
        if self.matches.is_empty() {
            frame.text.push(text(
                if library.is_empty() {
                    "No songs in the library folders"
                } else {
                    "No songs match"
                },
                inner_left,
                rows_top + row_h * 0.5 + size * 0.35,
                size,
                400,
                Ink::Faint,
            ));
        }

        // The footer: counts and folders on the left, the keys on the right.
        let small = c.body_size * 0.7;
        let footer = height - c.margin_bottom * 0.5;
        let folders: Vec<String> = library.folders.iter().map(|f| tilde(f)).collect();
        let summary = self.notice.clone().unwrap_or_else(|| {
            format!(
                "{} of {} songs  ·  {}",
                self.matches.len(),
                library.len(),
                folders.join(", ")
            )
        });
        let keys = "↑ ↓ move   ⏎ open   # tag   0–5 rate   tab sort   esc";
        let keys_w = w(keys, small, 400);
        let keys_fit = right - keys_w > left + w(&summary, small, 400) + small * 2.0;
        frame.text.push(text(
            &fit(
                &summary,
                if keys_fit {
                    right - keys_w - left - small * 2.0
                } else {
                    right - left
                },
                small,
                400,
                &w,
            ),
            left,
            footer,
            small,
            400,
            Ink::Faint,
        ));
        if keys_fit {
            frame
                .text
                .push(text(keys, right - keys_w, footer, small, 400, Ink::Faint));
        }
        frame
    }
}

/// The last space-separated word of `query`.
fn last_word(query: &str) -> &str {
    query.rsplit(' ').next().unwrap_or("")
}

fn rule(x: f32, y: f32, width: f32, ink: Ink) -> Shape {
    Shape {
        group: 0,
        x,
        y,
        width,
        height: 0.0,
        radius: 0.0,
        stroke: None,
        ink,
    }
}

/// `path` with the home folder as `~`.
fn tilde(path: &std::path::Path) -> String {
    let shown = path.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

/// `content` cut with an ellipsis to fit `width` points.
fn fit(
    content: &str,
    width: f32,
    size: f32,
    weight: u16,
    w: &impl Fn(&str, f32, u16) -> f32,
) -> String {
    if w(content, size, weight) <= width {
        return content.to_owned();
    }
    let chars: Vec<char> = content.chars().collect();
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_owned()
            + "…";
        if w(&candidate, size, weight) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low == 0 {
        return String::new();
    }
    chars[..low]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_owned()
        + "…"
}

/// A title or artist cell with its matched letters in bold. Each run is
/// placed after the measured width of everything before it.
#[allow(clippy::too_many_arguments)]
fn highlighted(
    frame: &mut Frame,
    content: &str,
    positions: &[usize],
    x: f32,
    width: f32,
    baseline: f32,
    size: f32,
    ink: Ink,
    w: &impl Fn(&str, f32, u16) -> f32,
) {
    let shown = fit(content, width, size, 400, w);
    let chars: Vec<char> = shown.chars().collect();
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (index, ch) in chars.iter().enumerate() {
        let hit = positions.contains(&index) && *ch != '…';
        match runs.last_mut() {
            Some((run, bold)) if *bold == hit => run.push(*ch),
            _ => runs.push((ch.to_string(), hit)),
        }
    }
    let mut at = x;
    for (run, bold) in runs {
        let weight = if bold { 700 } else { 400 };
        frame.text.push(Text {
            content: run.clone(),
            group: 0,
            x: at,
            baseline,
            size,
            weight,
            face: Face::System,
            ink: if bold { Ink::Strong } else { ink },
        });
        at += w(&run, size, weight);
    }
}

/// A metadata cell's text and ink.
fn cell(name: &str, entry: &crate::library::Entry, store: &Store) -> (String, Ink) {
    let dash = |value: &str| {
        if value.is_empty() {
            ("—".to_owned(), Ink::Faint)
        } else {
            (value.to_owned(), Ink::Soft)
        }
    };
    match name {
        "KEY" => dash(&entry.key),
        "CAPO" => dash(&entry.capo),
        "TEMPO" => dash(&entry.tempo),
        "YEAR" => dash(&entry.year),
        "★" => {
            let rating = store.rating(&entry.fingerprint) as usize;
            let stars = "★".repeat(rating) + &"☆".repeat(5 - rating);
            (stars, if rating == 0 { Ink::Faint } else { Ink::Soft })
        }
        _ => {
            let mut tags: Vec<String> = entry.file_tags.clone();
            for tag in store.tags(&entry.fingerprint) {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            let shown: Vec<String> = tags.iter().map(|t| format!("#{t}")).collect();
            (shown.join("  "), Ink::Soft)
        }
    }
}

/// Column widths in points. Metadata columns are as wide as their widest
/// value or header plus a gap; title and artist share what is left. When
/// they would get less than `min_title`, metadata columns drop, narrowest
/// value first in chordpro-tui's order: year, tempo, capo, tags, key, rating.
#[derive(Debug, Clone, PartialEq)]
struct Columns {
    /// (header, width) in display order; a dropped column is absent.
    columns: Vec<(&'static str, f32)>,
}

const ORDER: [&str; 8] = [
    "TITLE", "ARTIST", "KEY", "CAPO", "TEMPO", "YEAR", "★", "TAGS",
];
const DROP: [&str; 6] = ["YEAR", "TEMPO", "CAPO", "TAGS", "KEY", "★"];

impl Columns {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: f32,
        gap: f32,
        min_title: f32,
        library: &Library,
        store: &Store,
        matches: &[Match],
        value: impl Fn(&str, u16) -> f32,
        header: impl Fn(&str) -> f32,
    ) -> Self {
        let mut widths: Vec<(&'static str, f32)> = ORDER
            .iter()
            .skip(2)
            .map(|&name| {
                let widest = matches
                    .iter()
                    .map(|m| value(&cell(name, &library.entries[m.entry], store).0, 400))
                    .fold(header(name), f32::max);
                (name, widest + gap)
            })
            .collect();
        // Tags can be long; they take what is left, up to their widest.
        let flexible = |widths: &[(&str, f32)]| {
            width
                - widths
                    .iter()
                    .filter(|(n, _)| *n != "TAGS")
                    .map(|(_, w)| w)
                    .sum::<f32>()
        };
        for name in DROP {
            let tags = widths
                .iter()
                .find(|(n, _)| *n == "TAGS")
                .map_or(0.0, |(_, w)| *w);
            let tags_min = if tags > 0.0 {
                (header("TAGS") + gap).max(min_title * 0.5).min(tags)
            } else {
                0.0
            };
            if flexible(&widths) - tags_min >= 2.0 * min_title {
                break;
            }
            widths.retain(|(n, _)| *n != name);
        }
        let rest = flexible(&widths);
        let tags = widths
            .iter()
            .find(|(n, _)| *n == "TAGS")
            .map_or(0.0, |(_, w)| *w)
            .min((rest - 2.0 * min_title).max(0.0));
        let share = (rest - tags).max(0.0);
        let mut columns = vec![("TITLE", share * 0.58), ("ARTIST", share * 0.42)];
        for (name, w) in widths {
            columns.push((name, if name == "TAGS" { tags } else { w }));
        }
        columns.retain(|(_, w)| *w > 0.0);
        Self { columns }
    }

    /// Each column's header, left edge and width.
    fn visible(&self, left: f32) -> impl Iterator<Item = (&'static str, f32, f32)> + '_ {
        let mut x = left;
        self.columns.iter().map(move |&(name, width)| {
            let at = x;
            x += width;
            (name, at, width)
        })
    }

    #[cfg(test)]
    fn has(&self, name: &str) -> bool {
        self.columns.iter().any(|(n, _)| *n == name)
    }
}

/// chordpro-tui's `fuzzyMatch`: whether `query` is a case-insensitive
/// subsequence of `target`, with a score (word starts and runs rewarded,
/// later and longer targets penalised) and the matched character positions.
pub fn fuzzy_match(query: &str, target: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = target.to_lowercase().chars().collect();
    let (mut qi, mut prev, mut score) = (0usize, -2isize, 0i32);
    let mut positions = Vec::new();
    for ti in 0..t.len() {
        if qi == q.len() {
            break;
        }
        if t[ti] != q[qi] {
            continue;
        }
        if ti == 0 || matches!(t[ti - 1], ' ' | '-' | '_' | '.' | '/' | ',') {
            score += 8;
        } else if ti as isize == prev + 1 {
            score += 5;
        }
        score -= ti as i32 / 4;
        positions.push(ti);
        prev = ti as isize;
        qi += 1;
    }
    (qi == q.len()).then(|| (score + 20 - t.len() as i32, positions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Entry;
    use std::path::Path;

    fn library(songs: &[(&str, &str)]) -> Library {
        Library {
            folders: vec!["/songs".into()],
            entries: songs
                .iter()
                .map(|(name, source)| Entry::from_source(Path::new(name), source))
                .collect(),
        }
    }

    fn songs() -> Library {
        library(&[
            (
                "a.cho",
                "{title: House of the Rising Sun}\n{artist: Traditional}\n{key: Am}\n{tag: folk}\n[Am]There is a house in New Orleans",
            ),
            (
                "b.cho",
                "{title: Stolen Car}\n{artist: Beth Orton}\n{key: C}\n[C]hi",
            ),
            (
                "c.cho",
                "{title: Wagon Wheel}\n{artist: Old Crow Medicine Show}\n{year: 2004}\n[G]Headed down south",
            ),
            (
                "d.cho",
                "{title: Rise Up, Shepherd}\n{artist: Spiritual}\n[G]There's a star in the east",
            ),
        ])
    }

    fn titles(search: &Search, library: &Library) -> Vec<String> {
        search
            .results()
            .into_iter()
            .map(|i| library.entries[i].title.clone())
            .collect()
    }

    fn typed(search: &mut Search, text: &str, library: &Library, store: &Store) {
        for c in text.chars() {
            search.input(SearchInput::Char(c), library, store);
        }
    }

    // Ported from chordpro-tui internal/tui/picker_test.go.
    #[test]
    fn fuzzy_matches_subsequences_and_ranks_word_starts_higher() {
        assert!(fuzzy_match("wgn", "wagon_wheel.cho").is_some());
        assert_eq!(fuzzy_match("wh", "wagon_wheel.cho").unwrap().1.len(), 2);
        assert!(fuzzy_match("xyz", "wagon_wheel.cho").is_none());
        assert!(fuzzy_match("", "anything").is_some());
        let high = fuzzy_match("rs", "rising_sun.cho").unwrap().0;
        let low = fuzzy_match("rs", "characters.cho").unwrap().0;
        assert!(high > low, "{high} vs {low}");
    }

    #[test]
    fn a_query_filters_by_title_or_artist_and_ranks_title_hits_first() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        assert_eq!(search.results().len(), 4);
        assert_eq!(titles(&search, &library)[0], "House of the Rising Sun");
        typed(&mut search, "orton", &library, &store);
        assert_eq!(titles(&search, &library), ["Stolen Car"]);
        assert!(
            !search.matches[0].artist.is_empty(),
            "artist hit highlighted"
        );
        for _ in 0..5 {
            search.input(SearchInput::Backspace, &library, &store);
        }
        typed(&mut search, "ris", &library, &store);
        let found = titles(&search, &library);
        assert_eq!(found[..2], ["Rise Up, Shepherd", "House of the Rising Sun"]);
    }

    #[test]
    fn lyrics_are_searched_only_when_asked() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        typed(&mut search, "new orleans", &library, &store);
        assert!(search.results().is_empty());
        search.query.clear();
        typed(&mut search, "l:new orleans", &library, &store);
        assert_eq!(titles(&search, &library), ["House of the Rising Sun"]);
    }

    #[test]
    fn plus_and_star_words_become_filter_pills() {
        let (library, mut store) = (songs(), Store::default());
        let wheel = &library.entries[2];
        store.set_rating(&wheel.fingerprint, &wheel.title, 4);
        store.toggle_tag(&wheel.fingerprint, &wheel.title, "gig-set");
        let car = &library.entries[1];
        store.set_rating(&car.fingerprint, &car.title, 2);
        let mut search = Search::new(&library, &store);
        typed(&mut search, "*3", &library, &store);
        assert_eq!(search.min_rating, 3);
        assert_eq!(search.query, "");
        assert_eq!(titles(&search, &library), ["Wagon Wheel"]);
        search.input(SearchInput::Backspace, &library, &store);
        assert_eq!(search.min_rating, 0);
        typed(&mut search, "+Folk ", &library, &store);
        assert_eq!(search.tags, ["folk"]);
        assert_eq!(
            titles(&search, &library),
            ["House of the Rising Sun"],
            "file tags filter too"
        );
        search.input(SearchInput::Backspace, &library, &store);
        typed(&mut search, "+gig-set ", &library, &store);
        assert_eq!(
            titles(&search, &library),
            ["Wagon Wheel"],
            "sidecar tags filter"
        );
    }

    #[test]
    fn digits_rate_and_hash_tags_the_selected_song() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        search.input(SearchInput::Down, &library, &store);
        let selected = search.selected().unwrap();
        assert_eq!(
            search.input(SearchInput::Char('4'), &library, &store),
            SearchAction::Rate {
                entry: selected,
                rating: 4
            }
        );
        search.input(SearchInput::Char('#'), &library, &store);
        typed(&mut search, "Gig-Set", &library, &store);
        assert_eq!(search.tag_entry.as_deref(), Some("Gig-Set"));
        assert_eq!(
            search.input(SearchInput::Enter, &library, &store),
            SearchAction::Tag {
                entry: selected,
                tag: "gig-set".into()
            }
        );
        assert!(search.tag_entry.is_none());
        search.input(SearchInput::Char('#'), &library, &store);
        assert_eq!(
            search.input(SearchInput::Cancel, &library, &store),
            SearchAction::Redraw
        );
        assert_eq!(
            search.input(SearchInput::Cancel, &library, &store),
            SearchAction::Close
        );
    }

    #[test]
    fn a_rescan_keeps_the_selected_song_when_others_come_and_go() {
        let (old, store) = (songs(), Store::default());
        let mut search = Search::new(&old, &store);
        let rise = old
            .entries
            .iter()
            .position(|e| e.title == "Rise Up, Shepherd")
            .unwrap();
        search.select(rise);
        // A new song sorts first, shifting every index; one song is deleted.
        let mut new = old.clone();
        new.entries.remove(0);
        new.entries.insert(
            0,
            Entry::from_source(Path::new("a/Aardvark.cho"), "{title: Aardvark}"),
        );
        search.rescanned(&old, &new, &store);
        let selected = &new.entries[search.selected().unwrap()];
        assert_eq!(selected.title, "Rise Up, Shepherd");
        assert_eq!(search.results().len(), new.entries.len());
        assert!(titles(&search, &new).contains(&"Aardvark".to_owned()));
        // When the selected song itself goes, the best match is selected.
        let mut gone = new.clone();
        gone.entries.retain(|e| e.title != "Rise Up, Shepherd");
        search.rescanned(&new, &gone, &store);
        assert_eq!(search.selected(), Some(search.results()[0]));
    }

    #[test]
    fn enter_opens_the_rows_as_a_setlist_at_the_cursor() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        typed(&mut search, "e", &library, &store);
        search.input(SearchInput::Down, &library, &store);
        let rows = search.results();
        assert_eq!(
            search.input(SearchInput::Enter, &library, &store),
            SearchAction::Open {
                entries: rows,
                position: 1
            }
        );
    }

    #[test]
    fn sorting_cycles_and_rating_sorts_best_first() {
        let (library, mut store) = (songs(), Store::default());
        let rise = &library.entries[3];
        store.set_rating(&rise.fingerprint, &rise.title, 5);
        let mut search = Search::new(&library, &store);
        search.input(SearchInput::Sort, &library, &store);
        assert_eq!(search.sort, SortBy::Artist);
        assert_eq!(
            titles(&search, &library)[0],
            "Stolen Car",
            "Beth Orton first"
        );
        search.input(SearchInput::Sort, &library, &store);
        assert_eq!(titles(&search, &library)[0], "Rise Up, Shepherd");
        // Recent: newest first, then the never-shown by title.
        let (older, newer) = (&library.entries[0], &library.entries[2]);
        store.set_opened(&older.fingerprint, &older.title, 1_000);
        store.set_opened(&newer.fingerprint, &newer.title, 2_000);
        search.input(SearchInput::Sort, &library, &store);
        assert_eq!(search.sort, SortBy::Recent);
        let shown = titles(&search, &library);
        assert_eq!(&shown[..2], [newer.title.as_str(), older.title.as_str()]);
        let mut rest = shown[2..].to_vec();
        rest.sort_by_key(|t| t.to_lowercase());
        assert_eq!(shown[2..], rest[..]);
        search.input(SearchInput::Sort, &library, &store);
        assert_eq!(search.sort, SortBy::Title);
    }

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, text: &str, size: f32, _: u16, _: Face) -> f32 {
            text.chars().count() as f32 * size * 0.5
        }
    }

    #[test]
    fn columns_drop_narrowest_first_as_the_window_narrows() {
        let (library, store) = (songs(), Store::default());
        let search = Search::new(&library, &store);
        let columns = |width: f32| {
            Columns::new(
                width,
                12.0,
                160.0,
                &library,
                &store,
                &search.matches,
                |s, _| s.chars().count() as f32 * 7.5,
                |s| s.chars().count() as f32 * 6.0,
            )
        };
        let wide = columns(1040.0);
        assert!(ORDER.iter().all(|name| wide.has(name)), "{wide:?}");
        let narrow = columns(520.0);
        assert!(!narrow.has("YEAR") && narrow.has("TITLE") && narrow.has("ARTIST"));
        let titles = narrow.columns.iter().take(2).map(|(_, w)| w).sum::<f32>();
        assert!(titles >= 320.0 - 1e-3, "{narrow:?}");
        let tiny = columns(300.0);
        assert_eq!(tiny.columns.len(), 2, "{tiny:?}");
    }

    #[test]
    fn the_frame_draws_query_pills_header_and_highlighted_rows() {
        let (library, mut store) = (songs(), Store::default());
        let house = &library.entries[0];
        store.set_rating(&house.fingerprint, &house.title, 4);
        let mut search = Search::new(&library, &store);
        typed(&mut search, "ris *3", &library, &store);
        let frame = search.frame(
            1133.0,
            744.0,
            Polarity::Light,
            &library,
            &store,
            &Config::default(),
            &Fixed,
        );
        let has = |s: &str| frame.text.iter().any(|t| t.content == s);
        assert!(has("ris "), "query drawn");
        assert!(has("★ 3+  ×") && has("+ tag") && has("SORT  TITLE"));
        for header in ORDER {
            assert!(has(header), "{header} header");
        }
        // House of the Rising Sun: "Ris" in bold between plain runs.
        let bold: Vec<&Text> = frame
            .text
            .iter()
            .filter(|t| t.weight == 700 && t.size == Config::default().search_text_size)
            .collect();
        assert!(bold.iter().any(|t| t.content == "Ris"), "{bold:?}");
        assert!(has("★★★★☆"));
        assert!(has("1 of 4 songs  ·  /songs"));
        let pill = frame.text.iter().find(|t| t.content == "★ 3+  ×").unwrap();
        assert_eq!(pill.ink, Ink::Paper);
        assert!(
            frame.shapes.iter().any(|s| s.ink == Ink::Wash),
            "the selected row"
        );
    }
}
