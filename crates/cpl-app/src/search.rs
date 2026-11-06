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

use crate::{Config, Face, Frame, Ink, Measure, Polarity, Role, Shape, Text};
use crate::{library::Library, store::Store};

/// The scroll group of the result rows, which the frame's clip applies to.
const ROWS: usize = 1;

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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Query editing, as a macOS text field takes them: ← → (Ctrl-B, F);
    /// ⌘← ⌘→ (Ctrl-A, E) to either end; ⌥← ⌥→ by word; ⌥⌫ (Ctrl-W) a word
    /// back; ⌘⌫ (Ctrl-U) to the start; Ctrl-K to the end; ⌦ (Ctrl-D) forward.
    Left,
    Right,
    LineStart,
    LineEnd,
    WordLeft,
    WordRight,
    DeleteWord,
    DeleteToStart,
    DeleteToEnd,
    DeleteForward,
    /// ⌘A selects the whole query; ⇧ with an arrow (and ⌥ or ⌘) extends
    /// the selection as the arrow alone would move the caret. Typing,
    /// pasting or deleting replaces what is selected, and copy or cut
    /// takes it.
    SelectAll,
    SelectLeft,
    SelectRight,
    SelectWordLeft,
    SelectWordRight,
    SelectLineStart,
    SelectLineEnd,
    /// The pointer moved onto result row `n` (an index into the results).
    Focus(usize),
    /// A click on result row `n`: focus it and open it.
    OpenRow(usize),
    /// The scroll wheel or trackpad, in rows, fractions included so the
    /// list glides; positive moves down the list.
    Scroll(f32),
}

/// Where the result rows were last drawn, in page points, so the host can
/// hit-test the pointer without composing a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowsArea {
    pub left: f32,
    pub right: f32,
    /// The top of the first row drawn, which may sit above `visible.0` when
    /// the list is scrolled part way through a row.
    pub top: f32,
    pub row_height: f32,
    /// The band rows show in, top to bottom; rows are clipped to it.
    pub visible: (f32, f32),
    /// The result index of the first row drawn, and how many rows show.
    pub first: usize,
    pub shown: usize,
    /// The focused result when the frame was drawn.
    pub cursor: usize,
}

impl RowsArea {
    /// The result index under page point (`x`, `y`), if a row is there.
    pub fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < self.left || x >= self.right || y < self.visible.0 || y >= self.visible.1 {
            return None;
        }
        let row = ((y - self.top) / self.row_height) as usize;
        (row < self.shown).then_some(self.first + row)
    }
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
    /// Where typing goes in `query`, a byte offset; `None` is the end, so
    /// setting `query` directly leaves the caret after it.
    caret: Option<usize>,
    /// The other end of a selection in `query`, a byte offset; the selection
    /// runs between it and the caret.
    anchor: Option<usize>,
    cursor: usize,
    /// How far the list is scrolled, in rows; a fraction shows part of the
    /// top row.
    scroll: f32,
    matches: Vec<Match>,
    /// Where the last frame drew the rows; `None` before the first frame.
    area: Option<RowsArea>,
    /// Text widths already measured, by text, size and weight. Sizing the
    /// columns measures every result's cells, and a frame per key or wheel
    /// step re-measuring them all was most of its cost.
    widths: std::cell::RefCell<std::collections::HashMap<(String, u32, u16), f32>>,
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

    /// Where the last frame drew the result rows.
    pub fn rows_area(&self) -> Option<RowsArea> {
        self.area
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
        if input == SearchInput::SelectAll {
            self.anchor = Some(0);
            self.caret = None;
            return SearchAction::Redraw;
        }
        if let Some(at) = self.extend_to(input) {
            let caret = self.caret();
            self.anchor.get_or_insert(caret);
            self.caret = Some(at);
            return SearchAction::Redraw;
        }
        let selection = self.selection();
        if let Some(range) = selection.clone() {
            // An arrow collapses the selection to its end on that side;
            // an edit replaces it.
            match input {
                SearchInput::Left | SearchInput::Right => {
                    self.anchor = None;
                    self.caret = Some(if input == SearchInput::Left {
                        range.start
                    } else {
                        range.end
                    });
                    return SearchAction::Redraw;
                }
                SearchInput::Backspace
                | SearchInput::DeleteForward
                | SearchInput::DeleteWord
                | SearchInput::DeleteToStart
                | SearchInput::DeleteToEnd => {
                    self.delete_selection();
                    self.requery(library, store);
                    return SearchAction::Redraw;
                }
                SearchInput::Char(c) if !c.is_control() && c != '#' => {
                    self.delete_selection();
                    if c.is_ascii_digit() || c == ' ' {
                        // Typed over a selection, a digit or space is text.
                        let at = self.caret();
                        self.query.insert(at, c);
                        self.caret = Some(at + c.len_utf8());
                        self.requery(library, store);
                        return SearchAction::Redraw;
                    }
                }
                _ => {}
            }
        }
        if !matches!(input, SearchInput::Focus(_) | SearchInput::Scroll(_)) {
            self.anchor = None;
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
            SearchInput::Focus(row) => {
                if row < self.matches.len() {
                    self.cursor = row;
                }
            }
            SearchInput::OpenRow(row) => {
                if row < self.matches.len() {
                    self.cursor = row;
                    return SearchAction::Open {
                        entries: self.results(),
                        position: row,
                    };
                }
            }
            SearchInput::Scroll(rows) => self.scroll_by(rows),
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
                let at = self.caret();
                if at == 0 {
                    if self.query.is_empty() {
                        // An empty query backs into the pills, newest first.
                        if self.tags.pop().is_none() {
                            self.min_rating = 0;
                        }
                    }
                } else {
                    let from = prev_char(&self.query, at);
                    self.query.replace_range(from..at, "");
                    self.caret = Some(from);
                }
                self.requery(library, store);
            }
            SearchInput::Left => self.caret = Some(prev_char(&self.query, self.caret())),
            SearchInput::Right => self.caret = Some(next_char(&self.query, self.caret())),
            SearchInput::LineStart => self.caret = Some(0),
            SearchInput::LineEnd => self.caret = None,
            SearchInput::WordLeft => self.caret = Some(word_left(&self.query, self.caret())),
            SearchInput::WordRight => self.caret = Some(word_right(&self.query, self.caret())),
            SearchInput::DeleteWord => {
                let at = self.caret();
                let from = word_left(&self.query, at);
                self.query.replace_range(from..at, "");
                self.caret = Some(from);
                self.requery(library, store);
            }
            SearchInput::DeleteToStart => {
                let at = self.caret();
                self.query.replace_range(..at, "");
                self.caret = Some(0);
                self.requery(library, store);
            }
            SearchInput::DeleteToEnd => {
                let at = self.caret();
                self.query.truncate(at);
                self.requery(library, store);
            }
            SearchInput::DeleteForward => {
                let at = self.caret();
                let to = next_char(&self.query, at);
                self.query.replace_range(at..to, "");
                self.caret = Some(at);
                self.requery(library, store);
            }
            SearchInput::Char('#') => self.tag_entry = Some(String::new()),
            SearchInput::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                let at = self.caret();
                if self.query[..at].ends_with('*') {
                    // `*3` keeps three stars and up.
                    self.query.replace_range(at - 1..at, "");
                    self.caret = Some(at - 1);
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
            SearchInput::Char(' ') if last_word(&self.query[..self.caret()]).starts_with('+') => {
                let at = self.caret();
                let word = last_word(&self.query[..at]).to_owned();
                let from = at - word.len();
                self.query.replace_range(from..at, "");
                self.caret = Some(from);
                let tag = crate::library::normalise_tag(&word[1..]);
                if !tag.is_empty() && !self.tags.contains(&tag) {
                    self.tags.push(tag);
                }
                self.requery(library, store);
            }
            SearchInput::Char(c) if !(c.is_control() || c == ' ' && self.query.is_empty()) => {
                let at = self.caret();
                self.query.insert(at, c);
                self.caret = Some(at + c.len_utf8());
                self.requery(library, store);
            }
            _ => {}
        }
        SearchAction::Redraw
    }

    /// Where a selecting key moves the caret, or `None` for any other key.
    fn extend_to(&self, input: SearchInput) -> Option<usize> {
        let (text, at) = (&self.query, self.caret());
        Some(match input {
            SearchInput::SelectLeft => prev_char(text, at),
            SearchInput::SelectRight => next_char(text, at),
            SearchInput::SelectWordLeft => word_left(text, at),
            SearchInput::SelectWordRight => word_right(text, at),
            SearchInput::SelectLineStart => 0,
            SearchInput::SelectLineEnd => text.len(),
            _ => return None,
        })
    }

    /// The selected bytes of the query, when any are selected.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self
            .anchor
            .filter(|&at| at <= self.query.len() && self.query.is_char_boundary(at))?;
        let caret = self.caret();
        (anchor != caret).then(|| anchor.min(caret)..anchor.max(caret))
    }

    /// The selected text, else the whole query: what copy and cut take.
    pub fn copied_text(&self) -> &str {
        match self.selection() {
            Some(range) => &self.query[range],
            None => &self.query,
        }
    }

    fn delete_selection(&mut self) {
        if let Some(range) = self.selection() {
            self.query.replace_range(range.clone(), "");
            self.caret = Some(range.start);
        }
        self.anchor = None;
    }

    /// The caret as a byte offset into the query.
    pub fn caret(&self) -> usize {
        self.caret
            .filter(|&at| at <= self.query.len() && self.query.is_char_boundary(at))
            .unwrap_or(self.query.len())
    }

    /// Pastes `text` at the caret as typed text: line breaks become spaces,
    /// and digits stay in the query rather than rating a song.
    pub fn insert_text(&mut self, text: &str, library: &Library, store: &Store) {
        self.notice = None;
        let text: String = text
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        if let Some(tag) = &mut self.tag_entry {
            tag.extend(text.chars().filter(|c| *c != ' '));
            return;
        }
        self.delete_selection();
        let at = self.caret();
        self.query.insert_str(at, &text);
        self.caret = Some(at + text.len());
        self.requery(library, store);
    }

    /// Removes what cut copied: the selection, else the whole query.
    pub fn cut(&mut self, library: &Library, store: &Store) {
        if self.selection().is_some() {
            self.delete_selection();
        } else {
            self.query.clear();
            self.caret = None;
        }
        self.requery(library, store);
    }

    /// Scrolls the list without the cursor leading it; the cursor is
    /// pulled along only as far as it takes to stay on a visible row.
    fn scroll_by(&mut self, rows: f32) {
        let fits = self.area.map_or(1.0, |area| {
            (area.visible.1 - area.visible.0) / area.row_height
        });
        let most = (self.matches.len() as f32 - fits).max(0.0);
        self.scroll = (self.scroll + rows).clamp(0.0, most);
        // The cursor stays on a whole visible row.
        let first = (self.scroll - 1e-3).ceil().max(0.0) as usize;
        let last = ((self.scroll + fits + 1e-3).floor() as usize)
            .saturating_sub(1)
            .min(self.matches.len().saturating_sub(1));
        self.cursor = self.cursor.clamp(first, last.max(first));
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
        // The same song in the same file, else the file's first song.
        let entry = self.selected().and_then(|entry| {
            let old = &old.entries[entry];
            new.entries
                .iter()
                .position(|e| e.path == old.path && e.part == old.part)
                .or_else(|| new.entries.iter().position(|e| e.path == old.path))
        });
        self.rematch(new, store);
        match entry {
            Some(entry) => self.select(entry),
            None => self.scroll = 0.0,
        }
    }

    /// Recomputes the rows after the query or a filter changed, with the
    /// best match selected, as chordpro-tui's finder does.
    fn requery(&mut self, library: &Library, store: &Store) {
        self.rematch(library, store);
        self.scroll = 0.0;
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
            toggles: Vec::new(),
            clip: None,
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
            tracking: 0.0,
            ink,
        };
        let w = |content: &str, size: f32, weight: u16| {
            let mut widths = self.widths.borrow_mut();
            // A font size change leaves stale entries; bound the cache.
            if widths.len() > 20_000 {
                widths.clear();
            }
            *widths
                .entry((content.to_owned(), size.to_bits(), weight))
                .or_insert_with(|| measure.width(content, size, weight, Face::System))
        };

        // The query line: a magnifying glass (or `#` while a tag is typed),
        // the query with its caret, the hint under it, and a hairline.
        let q = c.search_query_size;
        let baseline = c.margin_top + q;
        let typed = match &self.tag_entry {
            Some(tag) => tag.as_str(),
            None => self.query.as_str(),
        };
        let query_x = match &self.tag_entry {
            Some(_) => {
                frame
                    .text
                    .push(text("#", left, baseline, q, 300, Ink::Soft));
                left + w("#", q, 300) + q * 0.5
            }
            None => {
                magnifier(&mut frame, left, baseline - q * 0.36, q);
                left + q * 1.25
            }
        };
        if let Some(range) = self.selection().filter(|_| self.tag_entry.is_none()) {
            // The selection sits on the highlighter, like a search match.
            let from = query_x + w(&typed[..range.start], q, 400);
            let to = query_x + w(&typed[..range.end], q, 400);
            frame.shapes.push(Shape {
                group: 0,
                x: from,
                y: baseline - q * 0.95,
                width: to - from,
                height: q * 1.15,
                radius: 2.0,
                stroke: None,
                ink: Ink::Highlight,
            });
        }
        frame
            .text
            .push(text(typed, query_x, baseline, q, 400, Ink::Strong));
        let caret_at = match &self.tag_entry {
            Some(tag) => tag.len(),
            None => self.caret(),
        };
        let caret_x = query_x
            + w(&typed[..caret_at], q, 400)
            + if caret_at == typed.len() {
                q * 0.15
            } else {
                0.0
            };
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
        let hint_size = c.search_label_size;
        let hint_baseline = baseline + hint_size * 1.9;
        frame.text.push(text(
            &fit(&hint, right - query_x, hint_size, 400, &w),
            query_x,
            hint_baseline,
            hint_size,
            400,
            Ink::Muted,
        ));
        let rule_y = hint_baseline + hint_size * 1.1;
        frame
            .shapes
            .push(rule(left, rule_y, right - left, Ink::Faint));

        // Filters: the label on its own line, then a pill per tag filter
        // and the rating floor, then hints for adding a tag and the sort.
        // The pills are the song page's metadata pills at the mock's body
        // size (where `Page/Meta` is the search label size): a soft outline,
        // a heavy soft label and a medium strong value, both tracked.
        let label = c.search_label_size;
        let label_baseline = rule_y + label * 2.2;
        frame.text.push(text(
            "FILTERS",
            left,
            label_baseline,
            label,
            600,
            Ink::Faint,
        ));
        let body = label / c.meta_size;
        let pill_h = c.pill_height * body;
        let pad = c.pill_pad * body;
        let kv_gap = c.pill_kv_gap * body;
        let style = |role| c.metrics().style(role);
        let (label_weight, value_weight) =
            (style(Role::PillLabel).weight, style(Role::PillValue).weight);
        let tracked = |content: &str, weight: u16| {
            w(content, label, weight) + content.chars().count() as f32 * c.meta_tracking * label
        };
        let pill_top = label_baseline + label * 0.8;
        let centre = pill_top + pill_h / 2.0;
        let pill_baseline = centre + 0.355 * label;
        let mut pills: Vec<(&str, String)> = self
            .tags
            .iter()
            .map(|tag| ("TAG", format!("{tag}  ×")))
            .collect();
        if self.min_rating > 0 {
            pills.push(("RATING", format!("★ {}+  ×", self.min_rating)));
        }
        pills.push(("+ TAG", String::new()));
        pills.push(("SORT", self.sort.label().to_owned()));
        let mut x = left;
        for (name, value) in pills {
            let name_w = tracked(name, label_weight);
            let value_w = if value.is_empty() {
                0.0
            } else {
                kv_gap + tracked(&value, value_weight)
            };
            let pill_w = 2.0 * pad + name_w + value_w;
            if x + pill_w > right {
                break;
            }
            frame.shapes.push(Shape {
                group: 0,
                x,
                y: pill_top,
                width: pill_w,
                height: pill_h,
                radius: pill_h / 2.0,
                stroke: Some(c.pill_stroke * body),
                ink: Ink::Soft,
            });
            let mut pill_text = |content: &str, at: f32, weight: u16, ink: Ink| {
                frame.text.push(Text {
                    tracking: c.meta_tracking,
                    ..text(content, at, pill_baseline, label, weight, ink)
                });
            };
            pill_text(name, x + pad, label_weight, Ink::Soft);
            if !value.is_empty() {
                pill_text(&value, x + pad + name_w + kv_gap, value_weight, Ink::Strong);
            }
            x += pill_w + c.pill_gap * body;
        }

        // The results box, its header row and as many rows as fit.
        let box_top = pill_top + pill_h + label * 1.8;
        let box_bottom = height - c.margin_bottom - c.status_height * c.scroll_size();
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
        // With no query the rows follow the sort, and its column's header
        // says which way: ↑ for A to Z, ↓ for best first.
        let sorted = self.query.trim().is_empty() && self.tag_entry.is_none();
        let header_text = |name: &str| -> String {
            match (sorted, self.sort, name) {
                (true, SortBy::Title, "TITLE") | (true, SortBy::Artist, "ARTIST") => {
                    format!("{name} ↑")
                }
                (true, SortBy::Rating, "RATING") => format!("{name} ↓"),
                _ => name.to_owned(),
            }
        };
        let columns = Columns::new(
            inner_right - inner_left,
            c.search_column_gap,
            c.search_min_title,
            c.search_cell_max,
            library,
            store,
            &self.matches,
            |s, weight| w(s, size, weight),
            |s| w(&header_text(s), label, 700),
        );
        let header_top = box_top + stroke;
        let header_baseline = header_top + c.search_header_row * 0.5 + label * 0.35;
        for (name, x, _) in columns.visible(inner_left) {
            frame.text.push(text(
                &header_text(name),
                x,
                header_baseline,
                label,
                700,
                Ink::Soft,
            ));
        }
        let rows_top = header_top + c.search_header_row;
        let row_h = c.search_row;
        let rows_bottom = (box_bottom - stroke).max(rows_top);
        let fits = (rows_bottom - rows_top) / row_h;
        // The keyboard's row is always whole on screen.
        let cursor = self.cursor as f32;
        if cursor < self.scroll {
            self.scroll = cursor;
        } else if fits >= 1.0 && cursor + 1.0 > self.scroll + fits {
            self.scroll = cursor + 1.0 - fits;
        }
        self.scroll = self
            .scroll
            .clamp(0.0, (self.matches.len() as f32 - fits).max(0.0));
        let first = self.scroll.floor() as usize;
        let first_top = rows_top - (self.scroll - first as f32) * row_h;
        let shown = (((rows_bottom - first_top) / row_h).ceil().max(0.0) as usize)
            .min(self.matches.len().saturating_sub(first));
        self.area = Some(RowsArea {
            left: left + stroke,
            right: right - stroke,
            top: first_top,
            row_height: row_h,
            visible: (rows_top, rows_bottom),
            first,
            shown,
            cursor: self.cursor,
        });
        let (texts, shapes) = (frame.text.len(), frame.shapes.len());
        for (row, m) in self.matches.iter().enumerate().skip(first).take(shown) {
            let top = first_top + (row - first) as f32 * row_h;
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
        // Rows scroll as one group, clipped to the band: the compositor
        // clips their text, and their shapes are trimmed here.
        for item in &mut frame.text[texts..] {
            item.group = ROWS;
        }
        frame.shapes.splice(
            shapes..,
            frame.shapes[shapes..]
                .iter()
                .filter_map(|shape| {
                    let y = shape.y.max(rows_top);
                    let bottom = (shape.y + shape.height).min(rows_bottom);
                    (bottom > y).then(|| Shape {
                        y,
                        height: bottom - y,
                        group: ROWS,
                        ..shape.clone()
                    })
                })
                .collect::<Vec<_>>(),
        );
        frame.clip = Some(crate::Rect {
            x: left,
            y: rows_top,
            width: right - left,
            height: rows_bottom - rows_top,
            group: ROWS,
        });
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
        let small = c.scroll_size() * 0.7;
        // On the page status strip's baseline.
        let footer = height - c.margin_bottom - c.status_drop * c.scroll_size();
        let folders: Vec<String> = library.folders.iter().map(|f| tilde(f)).collect();
        let summary = self.notice.clone().unwrap_or_else(|| {
            format!(
                "{} of {} songs  ·  {}",
                self.matches.len(),
                library.len(),
                folders.join(", ")
            )
        });
        // Each key is a keycap: a washed, outlined chip with the key in
        // strong ink, its action beside it in soft ink.
        let keys = [
            ("↑↓", "move"),
            ("⏎", "open"),
            ("#", "tag"),
            ("0–5", "rate"),
            ("tab", "sort"),
            ("esc", "close"),
        ];
        let cap_pad = small * 0.45;
        let cap_gap = small * 0.4;
        let key_gap = small * 1.2;
        let keys_w: f32 = keys
            .iter()
            .map(|(key, action)| {
                w(key, small, 600) + 2.0 * cap_pad + cap_gap + w(action, small, 400)
            })
            .sum::<f32>()
            + key_gap * (keys.len() - 1) as f32;
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
            Ink::Soft,
        ));
        if keys_fit {
            let mut x = right - keys_w;
            let cap_h = small * 1.55;
            let cap_y = footer - small * 0.35 - cap_h / 2.0;
            for (key, action) in keys {
                let cap_w = w(key, small, 600) + 2.0 * cap_pad;
                for stroke in [None, Some(1.0)] {
                    frame.shapes.push(Shape {
                        group: 0,
                        x,
                        y: cap_y,
                        width: cap_w,
                        height: cap_h,
                        radius: small * 0.3,
                        stroke,
                        ink: if stroke.is_some() {
                            Ink::Faint
                        } else {
                            Ink::Wash
                        },
                    });
                }
                frame
                    .text
                    .push(text(key, x + cap_pad, footer, small, 600, Ink::Strong));
                x += cap_w + cap_gap;
                frame
                    .text
                    .push(text(action, x, footer, small, 400, Ink::Soft));
                x += w(action, small, 400) + key_gap;
            }
        }
        frame
    }
}

fn prev_char(text: &str, at: usize) -> usize {
    text[..at].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_char(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(at, |c| at + c.len_utf8())
}

/// The start of the word before `at`, skipping spaces first, as ⌥← does.
fn word_left(text: &str, at: usize) -> usize {
    let before = text[..at].trim_end_matches(' ');
    before.rfind(' ').map_or(0, |i| i + 1)
}

/// The end of the word after `at`, skipping spaces first, as ⌥→ does.
fn word_right(text: &str, at: usize) -> usize {
    let after = &text[at..];
    let start = after.len() - after.trim_start_matches(' ').len();
    after[start..]
        .find(' ')
        .map_or(text.len(), |i| at + start + i)
}

/// A magnifying glass `size` points tall, its lens centred at (`left` plus
/// its radius, `centre_y`): a soft ring and a handle of overlapping discs
/// running down and right at 45°, since shapes are axis-aligned.
fn magnifier(frame: &mut Frame, left: f32, centre_y: f32, size: f32) {
    let radius = size * 0.3;
    let stroke = (size * 0.085).max(1.5);
    let cx = left + radius;
    frame.shapes.push(Shape {
        group: 0,
        x: cx - radius,
        y: centre_y - radius,
        width: radius * 2.0,
        height: radius * 2.0,
        radius,
        stroke: Some(stroke),
        ink: Ink::Soft,
    });
    let dot = stroke * 1.25;
    let start = radius - stroke * 0.3;
    let length = size * 0.3;
    let steps = 8;
    for step in 0..=steps {
        let along = (start + length * step as f32 / steps as f32) * std::f32::consts::FRAC_1_SQRT_2;
        frame.shapes.push(Shape {
            group: 0,
            x: cx + along - dot / 2.0,
            y: centre_y + along - dot / 2.0,
            width: dot,
            height: dot,
            radius: dot / 2.0,
            stroke: None,
            ink: Ink::Soft,
        });
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
        let run_w = w(&run, size, weight);
        if bold {
            // A marker stroke behind the match, from just above the
            // ascenders to just under the descenders.
            let bleed = size * 0.08;
            frame.shapes.push(Shape {
                group: 0,
                x: at - bleed,
                y: baseline - size * 0.92,
                width: run_w + 2.0 * bleed,
                height: size * 1.18,
                radius: size * 0.18,
                stroke: None,
                ink: Ink::Highlight,
            });
        }
        frame.text.push(Text {
            content: run.clone(),
            group: 0,
            x: at,
            baseline,
            size,
            weight,
            face: Face::System,
            tracking: 0.0,
            ink: if bold { Ink::Strong } else { ink },
        });
        at += run_w;
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
        "RATING" => {
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
    "TITLE", "ARTIST", "KEY", "CAPO", "TEMPO", "YEAR", "RATING", "TAGS",
];
const DROP: [&str; 6] = ["YEAR", "TEMPO", "CAPO", "TAGS", "KEY", "RATING"];

impl Columns {
    #[allow(clippy::too_many_arguments)]
    fn new(
        width: f32,
        gap: f32,
        min_title: f32,
        cell_max: f32,
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
                    .fold(0.0, f32::max);
                // A short value column stops at `cell_max`, so one long
                // note ("1 or 2 (Official: 0)") cannot crowd the title.
                let widest = match name {
                    "KEY" | "CAPO" | "TEMPO" | "YEAR" => widest.min(cell_max),
                    _ => widest,
                };
                (name, widest.max(header(name)) + gap)
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
    fn the_query_edits_like_a_mac_text_field() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        let key = |search: &mut Search, input| {
            search.input(input, &library, &store);
        };
        typed(&mut search, "rising sun", &library, &store);
        key(&mut search, SearchInput::WordLeft);
        assert_eq!(search.caret(), 7);
        key(&mut search, SearchInput::Left);
        key(&mut search, SearchInput::Char('!'));
        assert_eq!(search.query, "rising! sun");
        key(&mut search, SearchInput::LineStart);
        key(&mut search, SearchInput::DeleteForward);
        assert_eq!(search.query, "ising! sun");
        key(&mut search, SearchInput::WordRight);
        assert_eq!(search.caret(), 6);
        key(&mut search, SearchInput::DeleteWord);
        assert_eq!(search.query, " sun");
        key(&mut search, SearchInput::DeleteToEnd);
        assert_eq!(search.query, "");
        typed(&mut search, "house sun", &library, &store);
        key(&mut search, SearchInput::WordLeft);
        key(&mut search, SearchInput::DeleteToStart);
        assert_eq!((search.query.as_str(), search.caret()), ("sun", 0));
        key(&mut search, SearchInput::LineEnd);
        key(&mut search, SearchInput::Backspace);
        assert_eq!(search.query, "su");
    }

    #[test]
    fn a_paste_types_digits_and_lines_as_text() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        search.insert_text("rising\n1969", &library, &store);
        assert_eq!(search.query, "rising 1969");
        assert_eq!(search.caret(), search.query.len());
        search.cut(&library, &store);
        assert!(search.query.is_empty());
    }

    #[test]
    fn shift_arrows_and_command_a_select_and_an_edit_replaces_the_selection() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        let key = |search: &mut Search, input| {
            search.input(input, &library, &store);
        };
        search.insert_text("house sun", &library, &store);
        key(&mut search, SearchInput::SelectLeft);
        key(&mut search, SearchInput::SelectLeft);
        key(&mut search, SearchInput::SelectLeft);
        assert_eq!(search.copied_text(), "sun");
        // ← collapses to the selection's start, ⇧→ selects forward again.
        key(&mut search, SearchInput::Left);
        assert_eq!((search.selection(), search.caret()), (None, 6));
        key(&mut search, SearchInput::SelectRight);
        key(&mut search, SearchInput::SelectRight);
        assert_eq!(search.copied_text(), "su");
        // Typing replaces it, digits included.
        key(&mut search, SearchInput::Char('9'));
        assert_eq!(search.query, "house 9n");
        assert_eq!(search.selection(), None);
        // ⌘A, then backspace, empties the query; with nothing selected
        // copy takes the whole query.
        key(&mut search, SearchInput::SelectAll);
        assert_eq!(search.copied_text(), "house 9n");
        key(&mut search, SearchInput::Backspace);
        assert_eq!(search.query, "");
        search.insert_text("rise", &library, &store);
        assert_eq!(search.copied_text(), "rise");
        key(&mut search, SearchInput::SelectWordLeft);
        search.insert_text("sun", &library, &store);
        assert_eq!(search.query, "sun");
        // Cut takes only the selection.
        key(&mut search, SearchInput::LineStart);
        key(&mut search, SearchInput::SelectRight);
        search.cut(&library, &store);
        assert_eq!(search.query, "un");
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
                72.0,
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
    fn a_long_capo_note_stops_at_the_cell_limit_and_is_cut_short() {
        let mut library = songs();
        library.entries[0].capo = "1 or 2 (Official: 0)".into();
        let store = Store::default();
        let search = Search::new(&library, &store);
        let columns = Columns::new(
            1040.0,
            12.0,
            160.0,
            72.0,
            &library,
            &store,
            &search.matches,
            |s, _| s.chars().count() as f32 * 7.5,
            |s| s.chars().count() as f32 * 6.0,
        );
        let capo = columns
            .columns
            .iter()
            .find(|(n, _)| *n == "CAPO")
            .unwrap()
            .1;
        assert_eq!(capo, 72.0 + 12.0);
    }

    #[test]
    fn the_sorted_column_header_carries_its_arrow() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        let config = Config::default();
        let headers = |search: &mut Search| {
            let frame = search.frame(
                1100.0,
                700.0,
                Polarity::Light,
                &library,
                &store,
                &config,
                &Fixed,
            );
            frame
                .text
                .iter()
                .map(|t| t.content.clone())
                .filter(|t| ORDER.iter().any(|name| t.starts_with(name)))
                .collect::<Vec<_>>()
        };
        assert!(headers(&mut search).contains(&"TITLE ↑".to_owned()));
        search.input(SearchInput::Sort, &library, &store);
        let shown = headers(&mut search);
        assert!(shown.contains(&"ARTIST ↑".to_owned()) && shown.contains(&"TITLE".to_owned()));
        search.input(SearchInput::Sort, &library, &store);
        assert!(headers(&mut search).contains(&"RATING ↓".to_owned()));
        // A query ranks the rows instead, so no header claims the order.
        search.insert_text("ris", &library, &store);
        assert!(
            headers(&mut search)
                .iter()
                .all(|h| !h.contains('↑') && !h.contains('↓'))
        );
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
        assert!(has("RATING") && has("★ 3+  ×") && has("+ TAG"));
        assert!(has("SORT") && has("TITLE"));
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
        assert_eq!(pill.ink, Ink::Strong);
        assert_eq!(pill.tracking, Config::default().meta_tracking);
        assert!(
            frame.shapes.iter().any(|s| s.ink == Ink::Highlight),
            "the match is marked"
        );
        assert!(
            frame.shapes.iter().any(|s| s.ink == Ink::Wash),
            "the selected row"
        );
    }

    #[test]
    fn the_pointer_focuses_opens_and_scrolls_rows() {
        let (library, store) = (songs(), Store::default());
        let mut search = Search::new(&library, &store);
        // Short enough that only some of the four songs show.
        let draw = |search: &mut Search| {
            search.frame(
                600.0,
                330.0,
                Polarity::Light,
                &library,
                &store,
                &Config::default(),
                &Fixed,
            );
            search.rows_area().unwrap()
        };
        let area = draw(&mut search);
        assert!(area.shown >= 1 && area.shown < 4, "{area:?}");
        // A point in the first row hits it; one above the rows hits nothing.
        let y = area.top + area.row_height * 0.5;
        assert_eq!(area.row_at(area.left + 10.0, y), Some(0));
        assert_eq!(area.row_at(area.left + 10.0, area.top - 1.0), None);

        search.input(SearchInput::Focus(area.shown - 1), &library, &store);
        assert_eq!(search.cursor, area.shown - 1);

        // The wheel moves the view; the cursor follows only to stay visible.
        search.input(SearchInput::Scroll(10.0), &library, &store);
        let area = draw(&mut search);
        assert_eq!(area.first + area.shown, 4, "scrolled to the end: {area:?}");
        assert!(search.cursor >= area.first);
        search.input(SearchInput::Scroll(-10.0), &library, &store);
        assert_eq!(draw(&mut search).first, 0);

        let action = search.input(SearchInput::OpenRow(1), &library, &store);
        assert_eq!(
            action,
            SearchAction::Open {
                entries: search.results(),
                position: 1
            }
        );
        // Out of range, or while a tag is typed, the pointer does nothing.
        assert_eq!(
            search.input(SearchInput::OpenRow(9), &library, &store),
            SearchAction::Redraw
        );
    }
}
