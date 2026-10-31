//! Headless song-page composition. No window or GPU types cross this boundary.

pub mod library;
pub mod search;
mod setlist;
mod sha256;
pub mod store;

pub use cpl_layout::Tabs;
pub use cpl_layout::{Face, Measure};
use cpl_layout::{MeasuredSong, Metrics, Options, PageMetrics, Role, Solution, Viewport};
use cpl_song::{SectionKind, Song};
pub use library::{Entry, Library};
pub use search::{Search, SearchAction, SearchInput, SortBy};
pub use setlist::Setlist;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
pub use store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Strong,
    Soft,
    Faint,
    /// The paper itself, for text on a filled shape (a filter pill).
    Paper,
    /// A tint just off the paper, for the selected row.
    Wash,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub content: String,
    /// Items sharing a lyric line remain visible or hidden together in scroll mode.
    pub group: usize,
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    pub weight: u16,
    pub face: Face,
    /// Letter-spacing after every character, as a fraction of `size`.
    pub tracking: f32,
    pub ink: Ink,
}

/// A rounded rectangle in page points, filled or outlined: pill outlines,
/// chord-diagram grid lines and finger dots. Drawn beneath the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Scroll group, as for `Text`: a shape shows only with its whole group.
    pub group: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Corner radius; half the shorter side makes a capsule or a circle.
    pub radius: f32,
    /// `None` fills the shape; `Some(w)` draws an outline `w` points wide,
    /// inside the edge.
    pub stroke: Option<f32>,
    pub ink: Ink,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub polarity: Polarity,
    pub text: Vec<Text>,
    pub shapes: Vec<Shape>,
    pub fits: bool,
    pub columns: usize,
    pub ladder_step: u8,
}

/// One scroll step over a retained page: where the page sits in the window
/// and the band of it that shows only complete chord/lyric groups.
#[derive(Debug, Clone)]
pub struct ScrollView {
    /// The clamped page offset, in points.
    pub offset: f32,
    pub max_offset: f32,
    /// Window-space band, in points, outside which the host paints paper so
    /// a partially visible line never shows. Empty when nothing is visible.
    pub band: (f32, f32),
    /// The status strip of a scrolling page: where you are on the left, key
    /// hints on the right when there is room. A fitted page draws its own.
    pub status: Vec<Text>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub margin_x: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub title_size: f32,
    pub subtitle_size: f32,
    pub label_size: f32,
    pub chord_size: f32,
    /// The readability floor: body text is never smaller.
    pub body_size: f32,
    /// The size body text reads best at, and the size a scrolling page uses.
    pub comfort_size: f32,
    pub max_body_size: f32,
    /// What a point of body size is worth below and above `comfort_size`,
    /// and what each ladder step costs, in points (see `fit_solution`).
    pub size_loss_below: f32,
    pub size_gain_above: f32,
    pub cost_tight_sections: f32,
    pub cost_side_labels: f32,
    pub cost_diagrams: f32,
    pub cost_fold_tabs: f32,
    pub cost_pills: f32,
    pub cost_title: f32,
    pub cost_collapse_repeats: f32,
    pub cost_inline_chords: f32,
    pub header_gap: f32,
    pub section_gap: f32,
    pub line_height: f32,
    pub chord_baseline: f32,
    pub lyric_baseline: f32,
    pub label_height: f32,
    pub label_gap: f32,
    /// The section label's baseline below the top of its row, in em.
    pub label_baseline: f32,
    pub label_tracking: f32,
    pub chord_gap: f32,
    pub inline_line_height: f32,
    pub inline_lyric_baseline: f32,
    pub inline_chord_baseline: f32,
    pub inline_chord_size: f32,
    pub inline_chord_gap: f32,
    pub column_gutter: f32,
    pub byline_leading: f32,
    pub side_label_inset: f32,
    /// Indent of a wrapped line's continuation rows, in body sizes.
    pub wrap_indent: f32,
    /// Baseline advance between rows of a wrapped title, in title sizes.
    pub title_leading: f32,
    /// The first title row's baseline below the top margin, in body sizes.
    pub title_baseline: f32,
    /// The hairline under the title block: its drop below the block's last
    /// baseline and its thickness, in body sizes.
    pub header_rule_drop: f32,
    pub header_rule: f32,
    /// Metadata pills, in body sizes: text size and letter-spacing (a
    /// fraction of the text size), row and pill heights, inner padding,
    /// gaps between pills and between label and value, outline, and how far
    /// a pill beside the title block centres above its last baseline.
    pub meta_size: f32,
    pub meta_tracking: f32,
    pub pill_rise: f32,
    pub meta_row: f32,
    pub pill_height: f32,
    pub pill_pad: f32,
    pub pill_gap: f32,
    pub pill_kv_gap: f32,
    pub pill_stroke: f32,
    /// Chord diagrams, in body sizes: name and mark text, string and fret
    /// spacing, fret rows shown, the name, mark and finger rows, the gaps
    /// between diagrams, rows and above them, and the grid line, nut and
    /// dot sizes (chordpro-tui/internal/pdf/layout.go and pdf.go).
    pub diagram_name_size: f32,
    pub diagram_mark_size: f32,
    pub diagram_string_gap: f32,
    pub diagram_fret_gap: f32,
    pub diagram_frets: f32,
    pub diagram_name_row: f32,
    pub diagram_mark_row: f32,
    pub diagram_finger_row: f32,
    pub diagram_cell_gap: f32,
    pub diagram_row_gap: f32,
    pub diagram_top_gap: f32,
    pub diagram_line: f32,
    pub diagram_nut: f32,
    pub diagram_dot: f32,
    /// Tablature, in body sizes: text size, row height and the baseline in
    /// it, the gap between systems, the text's inset from the rule at its
    /// left, and the rule's width.
    pub tab_size: f32,
    pub tab_row: f32,
    pub tab_baseline: f32,
    pub tab_system_gap: f32,
    pub tab_inset: f32,
    pub tab_rule: f32,
    pub status_height: f32,
    /// Status strip text size, and its baseline above the bottom margin.
    pub status_size: f32,
    pub status_drop: f32,
    pub max_columns: f32,
    /// The search screen, in points: query, label and row text sizes, the
    /// header and row heights, pill height and padding, the gap after each
    /// column, the results box's outline and corner, and the narrowest the
    /// title and artist columns may get before metadata columns drop.
    pub search_query_size: f32,
    pub search_label_size: f32,
    pub search_text_size: f32,
    pub search_header_row: f32,
    pub search_row: f32,
    pub search_pill_height: f32,
    pub search_pill_pad: f32,
    pub search_column_gap: f32,
    pub search_box_stroke: f32,
    pub search_box_radius: f32,
    pub search_min_title: f32,
    /// Seconds between library rescans while search is open, so a song
    /// saved into a library folder appears without reopening search.
    pub search_rescan_seconds: f32,
}

const DEFAULT_CONFIG: &str = include_str!("../../../config/default.toml");

/// The lowest and highest readability floor `with_floor` accepts, in points.
pub const FLOOR_RANGE: std::ops::RangeInclusive<f32> = 6.0..=72.0;

impl Default for Config {
    fn default() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("bundled page config is valid")
    }
}

impl Config {
    /// Reads a complete numeric `[page]` table.
    pub fn parse(source: &str) -> Result<Self, String> {
        Self::from_values(Self::values(source)?)
    }

    /// The bundled defaults with a user's `[page]` table laid over them. The
    /// user's file names only the keys it changes; a key the defaults do not
    /// have is an error, so a typo never passes silently. Raising
    /// `body_size` (the readability floor) above the default largest size
    /// raises that too, unless the file sets `max_body_size` itself.
    pub fn with_overrides(user: &str) -> Result<Self, String> {
        let mut values = Self::values(DEFAULT_CONFIG)?;
        let overrides = Self::values(user)?;
        for (key, value) in &overrides {
            match values.get_mut(key) {
                Some(slot) => *slot = *value,
                None => return Err(format!("unknown page key: {key}")),
            }
        }
        if !overrides.contains_key("max_body_size") {
            let floor = values["body_size"];
            let max = values.get_mut("max_body_size").expect("a default key");
            *max = max.max(floor);
        }
        Self::from_values(values)
    }

    /// This config with the readability floor at `points`: body text is never
    /// set smaller. The largest size rises to meet a floor above it.
    pub fn with_floor(mut self, points: f32) -> Result<Self, String> {
        if !FLOOR_RANGE.contains(&points) {
            return Err(format!(
                "the floor must be {} to {} pt",
                FLOOR_RANGE.start(),
                FLOOR_RANGE.end()
            ));
        }
        self.body_size = points;
        self.max_body_size = self.max_body_size.max(points);
        Ok(self)
    }

    fn values(source: &str) -> Result<std::collections::BTreeMap<String, f32>, String> {
        let mut values = std::collections::BTreeMap::new();
        let mut in_page = false;
        for (index, raw) in source.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                if line != "[page]" {
                    return Err(format!("line {}: unsupported table", index + 1));
                }
                in_page = true;
                continue;
            }
            if !in_page {
                return Err(format!("line {}: expected [page]", index + 1));
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("line {}: expected key = value", index + 1))?;
            let key = key.trim();
            let value: f32 = value
                .trim()
                .parse()
                .map_err(|_| format!("line {}: expected number", index + 1))?;
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("line {}: value must be positive", index + 1));
            }
            if values.insert(key.to_owned(), value).is_some() {
                return Err(format!("line {}: duplicate {key}", index + 1));
            }
        }
        Ok(values)
    }

    fn from_values(mut values: std::collections::BTreeMap<String, f32>) -> Result<Self, String> {
        macro_rules! take {
            ($name:literal) => {
                values.remove($name).ok_or(concat!("missing ", $name))?
            };
        }
        let config = Self {
            margin_x: take!("margin_x"),
            margin_top: take!("margin_top"),
            margin_bottom: take!("margin_bottom"),
            title_size: take!("title_size"),
            subtitle_size: take!("subtitle_size"),
            label_size: take!("label_size"),
            chord_size: take!("chord_size"),
            body_size: take!("body_size"),
            comfort_size: take!("comfort_size"),
            max_body_size: take!("max_body_size"),
            size_loss_below: take!("size_loss_below"),
            size_gain_above: take!("size_gain_above"),
            cost_tight_sections: take!("cost_tight_sections"),
            cost_side_labels: take!("cost_side_labels"),
            cost_diagrams: take!("cost_diagrams"),
            cost_fold_tabs: take!("cost_fold_tabs"),
            cost_pills: take!("cost_pills"),
            cost_title: take!("cost_title"),
            cost_collapse_repeats: take!("cost_collapse_repeats"),
            cost_inline_chords: take!("cost_inline_chords"),
            header_gap: take!("header_gap"),
            section_gap: take!("section_gap"),
            line_height: take!("line_height"),
            chord_baseline: take!("chord_baseline"),
            lyric_baseline: take!("lyric_baseline"),
            label_height: take!("label_height"),
            label_gap: take!("label_gap"),
            label_baseline: take!("label_baseline"),
            label_tracking: take!("label_tracking"),
            chord_gap: take!("chord_gap"),
            inline_line_height: take!("inline_line_height"),
            inline_lyric_baseline: take!("inline_lyric_baseline"),
            inline_chord_baseline: take!("inline_chord_baseline"),
            inline_chord_size: take!("inline_chord_size"),
            inline_chord_gap: take!("inline_chord_gap"),
            column_gutter: take!("column_gutter"),
            byline_leading: take!("byline_leading"),
            side_label_inset: take!("side_label_inset"),
            wrap_indent: take!("wrap_indent"),
            title_leading: take!("title_leading"),
            title_baseline: take!("title_baseline"),
            header_rule_drop: take!("header_rule_drop"),
            header_rule: take!("header_rule"),
            meta_size: take!("meta_size"),
            meta_tracking: take!("meta_tracking"),
            pill_rise: take!("pill_rise"),
            meta_row: take!("meta_row"),
            pill_height: take!("pill_height"),
            pill_pad: take!("pill_pad"),
            pill_gap: take!("pill_gap"),
            pill_kv_gap: take!("pill_kv_gap"),
            pill_stroke: take!("pill_stroke"),
            diagram_name_size: take!("diagram_name_size"),
            diagram_mark_size: take!("diagram_mark_size"),
            diagram_string_gap: take!("diagram_string_gap"),
            diagram_fret_gap: take!("diagram_fret_gap"),
            diagram_frets: take!("diagram_frets"),
            diagram_name_row: take!("diagram_name_row"),
            diagram_mark_row: take!("diagram_mark_row"),
            diagram_finger_row: take!("diagram_finger_row"),
            diagram_cell_gap: take!("diagram_cell_gap"),
            diagram_row_gap: take!("diagram_row_gap"),
            diagram_top_gap: take!("diagram_top_gap"),
            diagram_line: take!("diagram_line"),
            diagram_nut: take!("diagram_nut"),
            diagram_dot: take!("diagram_dot"),
            tab_size: take!("tab_size"),
            tab_row: take!("tab_row"),
            tab_baseline: take!("tab_baseline"),
            tab_system_gap: take!("tab_system_gap"),
            tab_inset: take!("tab_inset"),
            tab_rule: take!("tab_rule"),
            status_height: take!("status_height"),
            status_size: take!("status_size"),
            status_drop: take!("status_drop"),
            max_columns: take!("max_columns"),
            search_query_size: take!("search_query_size"),
            search_label_size: take!("search_label_size"),
            search_text_size: take!("search_text_size"),
            search_header_row: take!("search_header_row"),
            search_row: take!("search_row"),
            search_pill_height: take!("search_pill_height"),
            search_pill_pad: take!("search_pill_pad"),
            search_column_gap: take!("search_column_gap"),
            search_box_stroke: take!("search_box_stroke"),
            search_box_radius: take!("search_box_radius"),
            search_min_title: take!("search_min_title"),
            search_rescan_seconds: take!("search_rescan_seconds"),
        };
        if let Some(key) = values.keys().next() {
            return Err(format!("unknown page key: {key}"));
        }
        if config.chord_baseline >= config.lyric_baseline
            || config.lyric_baseline >= config.line_height
            || config.inline_chord_baseline >= config.inline_lyric_baseline
            || config.inline_lyric_baseline >= config.inline_line_height
            || config.tab_baseline >= config.tab_row
        {
            return Err("invalid chord/lyric baseline order".into());
        }
        if config.max_columns.fract() != 0.0 {
            return Err("max_columns must be a whole number".into());
        }
        if config.max_body_size < config.body_size {
            return Err("max_body_size must meet the body_size floor".into());
        }
        Ok(config)
    }
}

impl Config {
    /// The size a scrolling page is set at: the comfortable size within the
    /// floor and the largest size, on a whole point so it was measured.
    pub fn scroll_size(&self) -> f32 {
        if self.comfort_size <= self.body_size {
            self.body_size
        } else if self.comfort_size >= self.max_body_size {
            self.max_body_size
        } else {
            self.comfort_size.floor().max(self.body_size)
        }
    }

    /// What a candidate's text size is worth, in points: full value up to
    /// the comfortable size, `size_gain_above` per point beyond it, and
    /// `size_loss_below` lost per point short of it.
    pub fn size_value(&self, size: f32) -> f32 {
        let comfort = self.comfort_size;
        if size >= comfort {
            comfort + (size - comfort) * self.size_gain_above
        } else {
            comfort - (comfort - size) * self.size_loss_below
        }
    }

    /// What a candidate's reductions cost, in points of body size. A fold
    /// the reader pinned with `t` (`tabs_pinned`) is their choice, not a cost.
    pub fn step_cost(&self, o: &Options, tabs_pinned: bool) -> f32 {
        [
            (o.tight_sections, self.cost_tight_sections),
            (o.side_labels, self.cost_side_labels),
            (o.hide_diagrams, self.cost_diagrams),
            (o.fold_tabs && !tabs_pinned, self.cost_fold_tabs),
            (o.hide_pills, self.cost_pills),
            (o.hide_title, self.cost_title),
            (o.collapse_repeats, self.cost_collapse_repeats),
            (o.inline_chords, self.cost_inline_chords),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, cost)| cost)
        .sum()
    }

    /// The type scale at the configured readability floor.
    pub fn metrics(&self) -> Metrics {
        self.metrics_at(self.body_size)
    }

    fn metrics_at(&self, body_size: f32) -> Metrics {
        Metrics {
            reference_size: body_size,
            title_size: self.title_size,
            subtitle_size: self.subtitle_size,
            label_size: self.label_size,
            chord_size: self.chord_size,
            chord_gap: self.chord_gap,
            inline_chord_size: self.inline_chord_size,
            inline_chord_gap: self.inline_chord_gap,
            meta_size: self.meta_size,
            meta_tracking: self.meta_tracking,
            label_tracking: self.label_tracking,
            diagram_name_size: self.diagram_name_size,
            diagram_mark_size: self.diagram_mark_size,
            tab_size: self.tab_size,
        }
    }
}

pub struct App {
    song: Song,
    config: Config,
    measured: MeasuredSong,
    larger: Vec<(f32, MeasuredSong)>,
    prepared: RefCell<Option<(u32, u32, Arc<PreparedPage>)>>,
    pub polarity: Polarity,
    /// The viewer's tablature choice; `set_tabs` changes it.
    tabs: Tabs,
    /// Each `HINTS` line's width at every body size the page may take, and a
    /// generous width per status character, in em of the body, so composing
    /// never measures. System text narrows as it grows, so one size is not
    /// enough to set the hints flush with the right margin.
    hint_widths: Vec<(f32, [f32; 3])>,
    status_char: f32,
}

/// The status strip's key hints, set at its right edge as in the mock.
#[derive(Clone, Copy)]
enum Hints {
    Fit,
    Scroll,
    Page,
}

const HINTS: [&str; 3] = [
    "[ ] transpose     v view     i invert     / search",
    "↑ ↓ scroll     v view     i invert     / search",
    "Space turn     v view     i invert     / search",
];

const STATUS_SEPARATOR: &str = "   ·   ";

/// Status text whose width stands in for any status line's, per character.
const STATUS_PROBE: &str = "FIT   ·   SCROLL   ·   PAGE 2 / 3   ·   stand 28 pt   ·   CHORUS";

enum PreparedPage {
    Fit(Frame),
    NeedsWidth(Frame),
    Scroll {
        content: Vec<Text>,
        shapes: Vec<Shape>,
        bounds: BTreeMap<usize, (f32, f32)>,
        sections: Vec<ScrollSection>,
        max_offset: f32,
        /// Paged mode's page starts: offsets at which a page begins on a
        /// whole group and holds every group that fits before the strip.
        pages: Vec<f32>,
    },
}

struct ScrollSection {
    groups: std::ops::Range<usize>,
    name: String,
}

#[derive(Clone, Copy)]
enum Position {
    Offset(f32),
    Page(usize),
}

/// Page starts over a single column of groups: each page begins at the top
/// of the first group the page before could not hold whole. A group taller
/// than a page (it cannot happen at the floor with wrapping, but a giant
/// title could) starts its own page and is cut, rather than looping.
fn page_starts(
    bounds: &BTreeMap<usize, (f32, f32)>,
    top_margin: f32,
    content_end: f32,
) -> Vec<f32> {
    let mut pages = vec![0.0];
    let mut offset = 0.0_f32;
    let page_height = content_end - top_margin;
    while let Some(&(top, _)) = bounds
        .values()
        .find(|&&(top, bottom)| bottom - offset > content_end && top - offset >= top_margin - 1e-3)
    {
        let mut next = top - top_margin;
        if next <= offset + 1e-3 {
            next = offset + page_height;
        }
        pages.push(next);
        offset = next;
    }
    pages
}

impl App {
    /// Measures the floor and each whole-point candidate up to the configured
    /// maximum. Resizing, scrolling and polarity then need no host measurement.
    pub fn new(song: Song, config: Config, polarity: Polarity, measure: &impl Measure) -> Self {
        let measured = cpl_layout::measure_song(&song, &config.metrics(), measure);
        let mut larger = Vec::new();
        let mut size = config.body_size.floor() + 1.0;
        while size <= config.max_body_size {
            larger.push((
                size,
                cpl_layout::measure_song(&song, &config.metrics_at(size), measure),
            ));
            size += 1.0;
        }
        if config.max_body_size > config.body_size
            && larger
                .last()
                .is_none_or(|(size, _)| *size < config.max_body_size)
        {
            larger.push((
                config.max_body_size,
                cpl_layout::measure_song(&song, &config.metrics_at(config.max_body_size), measure),
            ));
        }
        let status_pt = config.status_size * config.body_size;
        let hint_widths = std::iter::once(config.body_size)
            .chain(larger.iter().map(|(size, _)| *size))
            .map(|body| {
                let pt = config.status_size * body;
                (
                    body,
                    HINTS.map(|hint| measure.width(hint, pt, 400, Face::System) / body),
                )
            })
            .collect();
        let status_char = measure.width(STATUS_PROBE, status_pt, 400, Face::System)
            / STATUS_PROBE.chars().count() as f32
            / config.body_size
            * 1.15;
        Self {
            song,
            config,
            measured,
            larger,
            prepared: RefCell::new(None),
            polarity,
            tabs: Tabs::Auto,
            hint_widths,
            status_char,
        }
    }

    pub fn song(&self) -> &Song {
        &self.song
    }

    pub fn tabs(&self) -> Tabs {
        self.tabs
    }

    /// Pins tablature shown or folded, or hands it back to the ladder. The
    /// page is laid out again; nothing is measured.
    pub fn set_tabs(&mut self, tabs: Tabs) {
        if tabs != self.tabs {
            self.tabs = tabs;
            *self.prepared.borrow_mut() = None;
        }
    }

    /// Whether this viewport's page folds a tab the song has, so a toggle
    /// can unfold what is folded and fold what is shown.
    pub fn tabs_folded(&self, width: f32, height: f32) -> bool {
        self.song
            .sections
            .iter()
            .any(|section| section.kind == SectionKind::Tab)
            && match self.prepared(width, height).as_ref() {
                PreparedPage::Fit(_) | PreparedPage::NeedsWidth(_) => {
                    self.fit_solution(width, height).2.options.fold_tabs
                }
                PreparedPage::Scroll { .. } => self.tabs == Tabs::Fold,
            }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn page_metrics(&self, body: f32) -> PageMetrics {
        let c = &self.config;
        PageMetrics {
            title_size: c.title_size,
            title_baseline: c.title_baseline,
            subtitle_size: c.subtitle_size,
            byline_leading: c.byline_leading,
            header_gap: c.header_gap,
            section_gap: c.section_gap,
            label_height: c.label_height,
            label_gap: c.label_gap,
            line_height: c.line_height,
            inline_line_height: c.inline_line_height,
            column_gutter: c.column_gutter,
            side_label_room: c.margin_x.min(c.column_gutter * body) / body - c.side_label_inset,
            max_columns: c.max_columns as usize,
            wrap_indent: c.wrap_indent,
            title_leading: c.title_leading,
            meta_row: c.meta_row,
            pill_height: c.pill_height,
            pill_pad: c.pill_pad,
            pill_gap: c.pill_gap,
            pill_kv_gap: c.pill_kv_gap,
            meta_size: c.meta_size,
            pill_rise: c.pill_rise,
            header_rule_drop: c.header_rule_drop,
            diagram_string_gap: c.diagram_string_gap,
            diagram_fret_gap: c.diagram_fret_gap,
            diagram_frets: c.diagram_frets,
            diagram_name_row: c.diagram_name_row,
            diagram_mark_row: c.diagram_mark_row,
            diagram_finger_row: c.diagram_finger_row,
            diagram_cell_gap: c.diagram_cell_gap,
            diagram_row_gap: c.diagram_row_gap,
            diagram_top_gap: c.diagram_top_gap,
            tab_row: c.tab_row,
            tab_system_gap: c.tab_system_gap,
            tab_inset: c.tab_inset,
        }
    }

    /// The content box inside the margins and above the status strip, in em.
    fn viewport(&self, width: f32, height: f32, body: f32) -> Viewport {
        let c = &self.config;
        Viewport {
            width: (width - 2.0 * c.margin_x) / body,
            height: (height - c.margin_top - c.margin_bottom) / body - c.status_height,
        }
    }

    fn prepared(&self, width: f32, height: f32) -> Arc<PreparedPage> {
        let key = (width.to_bits(), height.to_bits());
        if self.prepared.borrow().as_ref().map(|(w, h, _)| (*w, *h)) != Some(key) {
            let page = self.prepare_page(width, height);
            *self.prepared.borrow_mut() = Some((key.0, key.1, Arc::new(page)));
        }
        Arc::clone(&self.prepared.borrow().as_ref().unwrap().2)
    }

    /// The page to retain for this viewport: a fitted page at window height
    /// (status strip included), or the whole scrolled song as one tall frame
    /// in page coordinates, without a status strip. `view` then positions it.
    pub fn page(&self, width: f32, height: f32) -> Frame {
        match self.prepared(width, height).as_ref() {
            PreparedPage::Fit(frame) | PreparedPage::NeedsWidth(frame) => {
                let mut frame = frame.clone();
                frame.polarity = self.polarity;
                frame
            }
            PreparedPage::Scroll {
                content,
                shapes,
                max_offset,
                ..
            } => Frame {
                width,
                height: height + max_offset,
                polarity: self.polarity,
                text: content.clone(),
                shapes: shapes.clone(),
                fits: false,
                columns: 1,
                ladder_step: 10,
            },
        }
    }

    /// Where the retained page sits for `offset`, and which band of it shows.
    /// Only the status strip's text changes per step, so this costs no
    /// measurement and no page composition.
    pub fn view(&self, width: f32, height: f32, offset: f32) -> ScrollView {
        self.view_at(width, height, Position::Offset(offset))
    }

    /// How many pages paged mode turns through at this viewport; 1 when the
    /// song fits.
    pub fn page_count(&self, width: f32, height: f32) -> usize {
        match self.prepared(width, height).as_ref() {
            PreparedPage::Scroll { pages, .. } => pages.len(),
            _ => 1,
        }
    }

    /// Paged mode's view of `page` (clamped to the last): the page begins on
    /// a whole chord/lyric group, even when that leaves the last page short,
    /// so a turn never repeats a line from the page before.
    pub fn page_view(&self, width: f32, height: f32, page: usize) -> ScrollView {
        self.view_at(width, height, Position::Page(page))
    }

    fn view_at(&self, width: f32, height: f32, position: Position) -> ScrollView {
        match self.prepared(width, height).as_ref() {
            PreparedPage::Fit(_) | PreparedPage::NeedsWidth(_) => ScrollView {
                offset: 0.0,
                max_offset: 0.0,
                band: (0.0, height),
                status: Vec::new(),
            },
            PreparedPage::Scroll {
                bounds,
                sections,
                max_offset,
                pages,
                ..
            } => {
                let c = &self.config;
                let body = self.scroll_body().0;
                let (offset, page) = match position {
                    Position::Offset(offset) => (offset.clamp(0.0, *max_offset), None),
                    Position::Page(page) => {
                        let page = page.min(pages.len() - 1);
                        (pages[page], Some(page))
                    }
                };
                let content_end = height - c.margin_bottom - c.status_height * body;
                let mut band = (f32::INFINITY, f32::NEG_INFINITY);
                let mut first_visible_group = None;
                for (&group, &(top, bottom)) in bounds {
                    if top - offset >= c.margin_top - 1e-3 && bottom - offset <= content_end {
                        band.0 = band.0.min(top - offset);
                        band.1 = band.1.max(bottom - offset);
                        if group != 0 {
                            first_visible_group =
                                Some(first_visible_group.map_or(group, |g: usize| g.min(group)));
                        }
                    }
                }
                // Neighbouring groups keep 0.45 em clear of each other, so a
                // little slack covers ascenders and descenders without
                // letting the next line's chord row peek in.
                let band = if band.0.is_finite() {
                    (band.0 - body * 0.15, band.1 + body * 0.15)
                } else {
                    (0.0, 0.0)
                };
                let content = match page {
                    None => self.scroll_status(
                        width,
                        offset,
                        *max_offset,
                        first_visible_group,
                        sections,
                    ),
                    Some(page) => {
                        self.page_status(width, page, pages.len(), first_visible_group, sections)
                    }
                };
                let hints = if page.is_some() {
                    Hints::Page
                } else {
                    Hints::Scroll
                };
                let status = self.status_texts(width, height, body, content, hints);
                ScrollView {
                    offset,
                    max_offset: *max_offset,
                    band,
                    status,
                }
            }
        }
    }

    /// The status strip: `left` at the margin and, when both fit with two em
    /// between them, the key hints ending at the right margin.
    fn status_texts(
        &self,
        width: f32,
        height: f32,
        body: f32,
        left: String,
        hints: Hints,
    ) -> Vec<Text> {
        let c = &self.config;
        let baseline = height - c.margin_bottom - c.status_drop * body;
        let text = |content: String, x: f32| Text {
            group: usize::MAX,
            content,
            x,
            baseline,
            size: c.status_size * body,
            weight: 400,
            face: Face::System,
            tracking: 0.0,
            ink: Ink::Soft,
        };
        let (_, widths) = self
            .hint_widths
            .iter()
            .min_by(|a, b| (a.0 - body).abs().total_cmp(&(b.0 - body).abs()))
            .expect("the floor is always measured");
        let hint_width = widths[hints as usize] * body;
        let left_width = left.chars().count() as f32 * self.status_char * body;
        let fits = left_width + 2.0 * body + hint_width <= width - 2.0 * c.margin_x;
        let mut status = vec![text(left, c.margin_x)];
        if fits {
            status.push(text(
                HINTS[hints as usize].to_owned(),
                width - c.margin_x - hint_width,
            ));
        }
        status
    }

    /// The key the player reads and the live transpose, as a status item
    /// with its leading separator: `Am`, or `Bm +2` once transposed. A
    /// narrow strip names it only when transposed.
    fn key_status(&self, separator: &str, wide: bool) -> String {
        let song = &self.song;
        let key = song
            .first_meta("key_actual")
            .or_else(|| song.first_meta("key"))
            .map(str::trim)
            .filter(|key| !key.is_empty());
        let offset = cpl_song::transpose::signed(song.transpose);
        match (key, song.transpose) {
            (Some(key), 0) if wide => format!("{separator}{key}"),
            (_, 0) => String::new(),
            (Some(key), _) => format!("{separator}{key} {offset}"),
            (None, _) => format!("{separator}transpose {offset}"),
        }
    }

    /// The section in view and a separator, three spaces either side in a
    /// wide window as in the mock, two in a narrow one.
    fn section_prefix(
        width: f32,
        first_visible_group: Option<usize>,
        sections: &[ScrollSection],
    ) -> String {
        let separator = if width < 700.0 {
            "  ·  "
        } else {
            STATUS_SEPARATOR
        };
        first_visible_group
            .and_then(|group| {
                sections
                    .iter()
                    .find(|section| section.groups.contains(&group))
            })
            .map_or(String::new(), |section| {
                format!("{}{separator}", section.name)
            })
    }

    fn page_status(
        &self,
        width: f32,
        page: usize,
        count: usize,
        first_visible_group: Option<usize>,
        sections: &[ScrollSection],
    ) -> String {
        let prefix = Self::section_prefix(width, first_visible_group, sections);
        let page = format!("PAGE {} / {count}", page + 1);
        if width < 700.0 {
            format!("{prefix}{page}{}", self.key_status("  ·  ", false))
        } else {
            format!(
                "{prefix}{page}{}{STATUS_SEPARATOR}stand {} pt",
                self.key_status(STATUS_SEPARATOR, true),
                self.scroll_body().0
            )
        }
    }

    fn scroll_status(
        &self,
        width: f32,
        offset: f32,
        max_offset: f32,
        first_visible_group: Option<usize>,
        sections: &[ScrollSection],
    ) -> String {
        let body = self.scroll_body().0;
        let more = if offset < max_offset {
            "  ·  MORE ↓"
        } else {
            ""
        };
        let wide_more = if offset < max_offset {
            format!("{STATUS_SEPARATOR}MORE ↓")
        } else {
            String::new()
        };
        let section_prefix = Self::section_prefix(width, first_visible_group, sections);
        if width < 400.0 {
            // A phone-narrow window keeps where you are and whether there is
            // more; the point size can wait.
            let more = if offset < max_offset { "  ·  ↓" } else { "" };
            let key = self.key_status("  ·  ", false);
            format!("{section_prefix}SCROLL{key}{more}")
        } else if width < 700.0 {
            let key = self.key_status("  ·  ", false);
            format!("{section_prefix}SCROLL{key}  ·  {body} pt{more}")
        } else {
            let key = self.key_status(STATUS_SEPARATOR, true);
            format!("{section_prefix}SCROLL{key}{STATUS_SEPARATOR}stand {body} pt{wide_more}")
        }
    }

    /// Scroll navigation selects a viewport from a retained, positioned page.
    pub fn frame_scrolled(&self, width: f32, height: f32, offset: f32) -> (Frame, f32) {
        let page = self.prepared(width, height);
        match page.as_ref() {
            PreparedPage::Fit(frame) | PreparedPage::NeedsWidth(frame) => {
                let mut frame = frame.clone();
                frame.polarity = self.polarity;
                (frame, 0.0)
            }
            PreparedPage::Scroll {
                content,
                shapes,
                bounds,
                sections,
                max_offset,
                ..
            } => {
                let c = &self.config;
                let body = self.scroll_body().0;
                let offset = offset.clamp(0.0, *max_offset);
                let content_end = height - c.margin_bottom - c.status_height * body;
                let visible = |group: usize| {
                    let (top, bottom) = bounds[&group];
                    top - offset >= c.margin_top && bottom - offset <= content_end
                };
                let mut frame = Frame {
                    width,
                    height,
                    polarity: self.polarity,
                    text: Vec::new(),
                    shapes: shapes
                        .iter()
                        .filter(|shape| visible(shape.group))
                        .map(|shape| Shape {
                            y: shape.y - offset,
                            ..shape.clone()
                        })
                        .collect(),
                    fits: false,
                    columns: 1,
                    ladder_step: 10,
                };
                let mut first_visible_group = None;
                for item in content {
                    let (top, bottom) = bounds[&item.group];
                    if top - offset >= c.margin_top && bottom - offset <= content_end {
                        if item.group != 0 {
                            first_visible_group = Some(
                                first_visible_group
                                    .map_or(item.group, |group: usize| group.min(item.group)),
                            );
                        }
                        let mut item = item.clone();
                        item.baseline -= offset;
                        frame.text.push(item);
                    }
                }
                frame.text.push(Text {
                    group: usize::MAX,
                    content: self.scroll_status(
                        width,
                        offset,
                        *max_offset,
                        first_visible_group,
                        sections,
                    ),
                    x: c.margin_x,
                    baseline: height - c.margin_bottom * 0.5,
                    size: body * 0.7,
                    weight: 400,
                    face: Face::Monospace,
                    tracking: 0.0,
                    ink: Ink::Faint,
                });
                (frame, *max_offset)
            }
        }
    }

    fn prepare_page(&self, width: f32, height: f32) -> PreparedPage {
        let mut fit = self.frame(width, height);
        if fit.fits {
            return PreparedPage::Fit(fit);
        }
        let c = &self.config;
        // A page that cannot fit at the floor scrolls at the comfortable size.
        let (body, measured) = self.scroll_body();
        let metrics = self.page_metrics(body);
        // One unbounded column; the scroll viewport windows over it.
        let tall = Viewport {
            height: f32::INFINITY,
            ..self.viewport(width, height, body)
        };
        // Lines and titles wrap at the content width, so only an unbreakable
        // run (a tab row, one enormous word) can still fail. A header is one
        // scroll group, and a group taller than the viewport could never be
        // shown whole, so a title that wraps past half the content box drops
        // here exactly as ladder step 7 drops it from a fitted page.
        // The header is one scroll group, so it must stay well inside the
        // viewport: past half of it, drop what the ladder drops first —
        // diagrams (step 4), then pills (step 6), then the title (step 7).
        let view_height = self.viewport(width, height, body).height;
        let mut base = Options {
            fold_tabs: self.tabs == Tabs::Fold,
            ..Options::default()
        };
        let too_tall = |o: &Options| {
            o.header(&measured.header, &metrics, tall.width).height > view_height / 2.0
        };
        if too_tall(&base) {
            base.hide_diagrams = true;
        }
        if too_tall(&base) {
            base.hide_pills = true;
        }
        if too_tall(&base) {
            base.hide_title = true;
        }
        let mut long = cpl_layout::layout(measured, &metrics, tall, base, 0);
        if !long.fit.meets_floor {
            let inline = Options {
                inline_chords: true,
                ..base
            };
            long = cpl_layout::layout(measured, &metrics, tall, inline, 9);
        }
        // A single title word wider than the window still clips at its edge
        // rather than withholding the song.
        long.fits = long.fit.meets_floor;
        if !long.fits {
            for item in &mut fit.text {
                if item.content == "Song needs paging at this readable size" {
                    item.content = "Widen window to show this song".into();
                } else if item.content.starts_with("PAGE NEEDED") {
                    item.content = format!("WIDTH NEEDED{STATUS_SEPARATOR}stand {} pt", body);
                }
            }
            return PreparedPage::NeedsWidth(fit);
        }
        let long_options = long.options;
        let mut long = self.compose(width, height, body, measured, &long);
        // The long page's status strip belongs below its viewport.
        long.text.retain(|item| item.group != usize::MAX);
        let content_end = height - c.margin_bottom - c.status_height * body;
        let last_baseline = long
            .text
            .iter()
            .map(|item| item.baseline)
            .fold(c.margin_top, f32::max);
        let max_offset = (last_baseline + body * 0.5 - content_end).max(0.0);
        let mut group_bounds = BTreeMap::<usize, (f32, f32)>::new();
        for item in &long.text {
            let bounds = group_bounds
                .entry(item.group)
                .or_insert((f32::INFINITY, f32::NEG_INFINITY));
            bounds.0 = bounds.0.min(item.baseline - item.size);
            bounds.1 = bounds.1.max(item.baseline + item.size * 0.25);
        }
        for shape in &long.shapes {
            let bounds = group_bounds
                .entry(shape.group)
                .or_insert((f32::INFINITY, f32::NEG_INFINITY));
            bounds.0 = bounds.0.min(shape.y);
            bounds.1 = bounds.1.max(shape.y + shape.height);
        }
        // A text's top is estimated as its baseline less its size, which for
        // the title (set on the mock's baseline, above a full em) lands above
        // the top margin. The header starts at the margin by construction,
        // and a scroll at the top must show it.
        if let Some(header) = group_bounds.get_mut(&0) {
            header.0 = header.0.max(c.margin_top);
        }
        let mut first_group = 1;
        let sections = self
            .measured
            .sections
            .iter()
            .map(|section| {
                let count = long_options.groups(section, &metrics, tall.width);
                let name = if section.label.is_empty() {
                    match section.kind {
                        SectionKind::Verse => "VERSE",
                        SectionKind::Chorus => "CHORUS",
                        SectionKind::Bridge => "BRIDGE",
                        SectionKind::Tab => "TAB",
                        SectionKind::Unlabelled => "SONG",
                    }
                    .to_owned()
                } else {
                    section.label.clone()
                };
                let marker = ScrollSection {
                    groups: first_group..first_group + count,
                    name,
                };
                first_group += count;
                marker
            })
            .collect();
        let pages = page_starts(&group_bounds, c.margin_top, content_end);
        PreparedPage::Scroll {
            content: long.text,
            shapes: long.shapes,
            bounds: group_bounds,
            sections,
            max_offset,
            pages,
        }
    }

    /// The least-reduced page that fits, or the most-reduced candidate marked unfit.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let (body, measured, solution) = self.fit_solution(width, height);
        self.compose(width, height, body, measured, &solution)
    }

    /// The fitted page's body size, measurement and layout. Each ladder
    /// candidate that fits at the floor is drawn at the largest size that
    /// keeps it, then weighed: its size's worth (`Config::size_value`) less
    /// the price of every step it took (`Config::step_cost`). The best wins,
    /// the less reduced on a tie, so text shrinks some way toward the floor
    /// before the page gives anything up. When nothing fits at the floor,
    /// the most reduced candidate is returned unfit and the page scrolls.
    fn fit_solution(&self, width: f32, height: f32) -> (f32, &MeasuredSong, Solution) {
        let c = &self.config;
        let floor = c.body_size;
        let metrics = self.page_metrics(floor);
        let view = self.viewport(width, height, floor);
        let candidates = cpl_layout::candidates(&self.measured, &metrics, view, self.tabs);
        let mut best: Option<(f32, f32, &MeasuredSong, Solution)> = None;
        let mut last = None;
        for &(step, options) in &candidates {
            let at_floor = cpl_layout::layout(&self.measured, &metrics, view, options, step);
            if !at_floor.fits {
                last = Some(at_floor);
                continue;
            }
            let (body, measured, solution) = self.largest_keeping(width, height, at_floor);
            let score = c.size_value(body) - c.step_cost(&options, self.tabs != Tabs::Auto);
            if best.as_ref().is_none_or(|b| score > b.0 + 1e-4) {
                best = Some((score, body, measured, solution));
            }
            // Later steps cost more and cannot grow past the largest size.
            if body >= c.max_body_size {
                break;
            }
        }
        match best {
            Some((_, body, measured, solution)) => (body, measured, solution),
            None => (
                floor,
                &self.measured,
                last.expect("the full page is a candidate"),
            ),
        }
    }

    /// `at_floor` drawn at the largest measured size that still fits with
    /// the same reductions. Bigger type is only better while it wraps no more
    /// lines than the floor does: a line broken in three at 28 pt reads
    /// worse than the same line whole at 20 pt.
    fn largest_keeping(
        &self,
        width: f32,
        height: f32,
        at_floor: Solution,
    ) -> (f32, &MeasuredSong, Solution) {
        for (body, measured) in self.larger.iter().rev() {
            let larger = cpl_layout::layout(
                measured,
                &self.page_metrics(*body),
                self.viewport(width, height, *body),
                at_floor.options,
                at_floor.step,
            );
            if larger.fits && larger.wrapped_rows <= at_floor.wrapped_rows {
                return (*body, measured, larger);
            }
        }
        (self.config.body_size, &self.measured, at_floor)
    }

    /// The size and measurement a scrolling page is set at.
    fn scroll_body(&self) -> (f32, &MeasuredSong) {
        let size = self.config.scroll_size();
        self.larger
            .iter()
            .find(|(body, _)| (body - size).abs() < 1e-3)
            .map_or((self.config.body_size, &self.measured), |(body, m)| {
                (*body, m)
            })
    }

    /// One fingering diagram with its cell's top-left corner at `at`, in
    /// points: name, o/x marks, grid with nut or starting-fret note, dots and
    /// barres, finger numbers. After chordpro-tui's internal/pdf/pdf.go
    /// drawDiagram; barres are drawn as capsules, as in the Figma mock.
    fn compose_diagram(
        &self,
        frame: &mut Frame,
        m: &PageMetrics,
        diagram: &cpl_layout::MeasuredDiagram,
        (left, top): (f32, f32),
        body: f32,
        mark_width: f32,
    ) {
        let c = &self.config;
        let shape = &diagram.shape;
        let strings = shape.frets.len();
        let gap = c.diagram_string_gap * body;
        let grid_width = (strings - 1) as f32 * gap;
        let label_room = diagram
            .fret_label
            .as_ref()
            .map_or(0.0, |(_, w)| (w + 0.15) * body);
        let cell = m.diagram_width(diagram) * body - label_room;
        let grid_left = left + (cell - grid_width) / 2.0;
        let string_x = |i: usize| grid_left + i as f32 * gap;
        let small = |content: String, x: f32, baseline: f32| Text {
            group: 0,
            content,
            x,
            baseline,
            size: c.diagram_mark_size * body,
            weight: 400,
            face: Face::Monospace,
            tracking: 0.0,
            ink: Ink::Soft,
        };
        let line = |x: f32, y: f32, width: f32, height: f32, radius: f32| Shape {
            group: 0,
            x,
            y,
            width,
            height,
            radius,
            stroke: None,
            ink: Ink::Strong,
        };
        frame.text.push(Text {
            group: 0,
            content: diagram.name.clone(),
            x: grid_left + grid_width / 2.0 - diagram.name_width * body / 2.0,
            baseline: top + 0.75 * c.diagram_name_row * body,
            size: c.diagram_name_size * body,
            weight: 600,
            face: Face::Monospace,
            tracking: 0.0,
            ink: Ink::Strong,
        });
        let marks = top + c.diagram_name_row * body;
        let mark_half = mark_width * body / 2.0;
        for (i, &fret) in shape.frets.iter().enumerate() {
            let mark = match fret {
                f if f < 0 => "x",
                0 => "o",
                _ => continue,
            };
            frame.text.push(small(
                mark.into(),
                string_x(i) - mark_half,
                marks + 0.34 * body,
            ));
        }
        let grid_top = marks + c.diagram_mark_row * body;
        let fret_gap = c.diagram_fret_gap * body;
        let grid_height = c.diagram_frets * fret_gap;
        let stroke = c.diagram_line * body;
        for i in 0..strings {
            frame.shapes.push(line(
                string_x(i) - stroke / 2.0,
                grid_top,
                stroke,
                grid_height,
                0.0,
            ));
        }
        for row in 0..=c.diagram_frets as usize {
            let y = grid_top + row as f32 * fret_gap;
            frame
                .shapes
                .push(line(grid_left, y - stroke / 2.0, grid_width, stroke, 0.0));
        }
        if shape.base_fret <= 1 {
            let nut = c.diagram_nut * body;
            frame
                .shapes
                .push(line(grid_left, grid_top - nut / 2.0, grid_width, nut, 0.0));
        }
        let dot = c.diagram_dot * body;
        let row_centre = |fret: i8| grid_top + (fret as f32 - 0.5) * fret_gap;
        for &barre in &shape.barres {
            let held: Vec<usize> = (0..strings)
                .filter(|&i| shape.frets[i] == barre as i8)
                .collect();
            if let (Some(&first), Some(&last)) = (held.first(), held.last())
                && last > first
            {
                let y = row_centre(barre as i8);
                frame.shapes.push(line(
                    string_x(first) - dot,
                    y - dot,
                    string_x(last) - string_x(first) + 2.0 * dot,
                    2.0 * dot,
                    dot,
                ));
            }
        }
        for (i, &fret) in shape.frets.iter().enumerate() {
            if fret > 0 && fret as f32 <= c.diagram_frets {
                let y = row_centre(fret);
                frame
                    .shapes
                    .push(line(string_x(i) - dot, y - dot, 2.0 * dot, 2.0 * dot, dot));
            }
        }
        if let Some((label, _)) = &diagram.fret_label {
            frame.text.push(small(
                label.clone(),
                grid_left + grid_width + 0.15 * body,
                grid_top + fret_gap * 0.65,
            ));
        }
        let fingers = grid_top + grid_height + 0.45 * body;
        for (i, &finger) in shape.fingers.iter().enumerate().take(strings) {
            if finger > 0 {
                frame
                    .text
                    .push(small(finger.to_string(), string_x(i) - mark_half, fingers));
            }
        }
    }

    /// Places a solved candidate in points. Draws no lyric unless it fits.
    fn compose(
        &self,
        width: f32,
        height: f32,
        body: f32,
        measured: &MeasuredSong,
        solution: &Solution,
    ) -> Frame {
        let c = &self.config;
        let o = solution.options;
        let m = self.page_metrics(body);
        let header = &measured.header;
        let mut frame = Frame {
            width,
            height,
            polarity: self.polarity,
            text: Vec::new(),
            shapes: Vec::new(),
            fits: solution.fits,
            columns: if solution.fits {
                solution.fit.columns.len()
            } else {
                0
            },
            ladder_step: solution.step,
        };
        for (row, baseline) in &solution.header.title_rows {
            frame.text.push(Text {
                group: 0,
                content: row.clone(),
                x: c.margin_x,
                baseline: c.margin_top + baseline * body,
                size: c.title_size * solution.header.title_scale * body,
                weight: 700,
                face: Face::System,
                tracking: 0.0,
                ink: Ink::Strong,
            });
        }
        if let (Some(artist), Some((x, baseline))) = (&header.artist, solution.header.artist) {
            frame.text.push(Text {
                group: 0,
                content: artist.clone(),
                x: c.margin_x + x * body,
                baseline: c.margin_top + baseline * body,
                size: c.subtitle_size * body,
                weight: 400,
                face: Face::System,
                tracking: 0.0,
                ink: Ink::Soft,
            });
        }
        for (pill, &(x, centre)) in header.pills.iter().zip(&solution.header.pills) {
            let left = c.margin_x + x * body;
            let centre = c.margin_top + centre * body;
            frame.shapes.push(Shape {
                group: 0,
                x: left,
                y: centre - c.pill_height * body / 2.0,
                width: m.pill_width(pill) * body,
                height: c.pill_height * body,
                radius: c.pill_height * body / 2.0,
                stroke: Some(c.pill_stroke * body),
                ink: Ink::Soft,
            });
            // The mock centres a 21 pt line box in the pill; for SF Pro that
            // puts the baseline 0.355 of the text size below the middle.
            let baseline = centre + 0.355 * c.meta_size * body;
            let style = |role| c.metrics().style(role);
            frame.text.push(Text {
                group: 0,
                content: pill.label.clone(),
                x: left + c.pill_pad * body,
                baseline,
                size: c.meta_size * body,
                weight: style(Role::PillLabel).weight,
                face: Face::System,
                tracking: c.meta_tracking,
                ink: Ink::Soft,
            });
            frame.text.push(Text {
                group: 0,
                content: pill.value.clone(),
                x: left + (c.pill_pad + pill.label_width + c.pill_kv_gap) * body,
                baseline,
                size: c.meta_size * body,
                weight: style(Role::PillValue).weight,
                face: Face::System,
                tracking: c.meta_tracking,
                ink: Ink::Strong,
            });
        }
        if let Some(rule) = solution.header.rule {
            frame.shapes.push(Shape {
                group: 0,
                x: c.margin_x,
                y: c.margin_top + rule * body - c.header_rule * body,
                width: width - 2.0 * c.margin_x,
                height: c.header_rule * body,
                radius: 0.0,
                stroke: None,
                ink: Ink::Faint,
            });
        }
        for (diagram, &(x, top)) in header.diagrams.iter().zip(&solution.header.diagrams) {
            self.compose_diagram(
                &mut frame,
                &m,
                diagram,
                (c.margin_x + x * body, c.margin_top + top * body),
                body,
                header.mark_width,
            );
        }
        let y = c.margin_top + solution.header.height * body;
        let content_end = height - c.margin_bottom - c.status_height * body;
        let wrap = self.viewport(width, height, body).width;
        if frame.fits {
            let metrics = c.metrics();
            let label_style = metrics.style(Role::Label);
            let mut column_x = c.margin_x;
            let mut line_group = 1;
            for range in &solution.fit.columns {
                let mut section_y = y;
                for (offset, index) in range.clone().enumerate() {
                    let section = &measured.sections[index];
                    if offset > 0 {
                        section_y += o.section_gap(&m) * body;
                    }
                    let inline = o.inline(section);
                    if !section.label.is_empty() {
                        if !o.side_labels {
                            section_y += c.label_height * body;
                        }
                        frame.text.push(Text {
                            group: line_group,
                            content: section.label.clone(),
                            x: if o.side_labels {
                                column_x - (section.label_width + c.side_label_inset) * body
                            } else {
                                column_x
                            },
                            baseline: if o.side_labels && o.tab_shown(section) {
                                section_y + c.tab_baseline * body
                            } else if o.side_labels {
                                section_y
                                    + (if section.lines.first().is_some_and(|line| line.chord_only)
                                        || inline
                                    {
                                        c.inline_lyric_baseline
                                    } else {
                                        c.lyric_baseline
                                    }) * body
                            } else {
                                section_y - (c.label_height - c.label_baseline) * body
                            },
                            size: label_style.size * body,
                            weight: label_style.weight,
                            face: label_style.face,
                            tracking: label_style.tracking,
                            // The mock sets every label in the soft ink; a
                            // chorus label is the strong ink, so the part
                            // that comes back is easy to find.
                            ink: if section.kind == SectionKind::Chorus {
                                Ink::Strong
                            } else {
                                Ink::Soft
                            },
                        });
                        if o.tab_folded(section) {
                            // The cue says the tab is there and folded, so an
                            // empty label never reads as a missing section.
                            let label = frame.text.last().unwrap().clone();
                            let (content, x) = if o.side_labels {
                                (cpl_layout::FOLD_CUE.to_owned(), column_x)
                            } else {
                                (
                                    format!(
                                        "{}{}",
                                        cpl_layout::FOLD_SEPARATOR,
                                        cpl_layout::FOLD_CUE
                                    ),
                                    label.x + section.label_width * body,
                                )
                            };
                            frame.text.push(Text {
                                content,
                                x,
                                ink: Ink::Faint,
                                ..label
                            });
                        }
                        if !o.side_labels {
                            section_y += c.label_gap * body;
                        }
                    }
                    if o.tab_shown(section) {
                        // Each system is one group: a rule down its left and
                        // its rows, small and soft, so the tab reads as a
                        // part for the player and never outweighs the words.
                        for (index, system) in o.tab_systems(section, &m, wrap).iter().enumerate() {
                            if index > 0 {
                                section_y += c.tab_system_gap * body;
                            }
                            let top = section_y;
                            for row in &system.rows {
                                if !row.is_empty() {
                                    frame.text.push(Text {
                                        group: line_group,
                                        content: row.clone(),
                                        x: column_x + c.tab_inset * body,
                                        baseline: section_y + c.tab_baseline * body,
                                        size: c.tab_size * body,
                                        weight: 400,
                                        face: Face::Monospace,
                                        tracking: 0.0,
                                        ink: Ink::Soft,
                                    });
                                }
                                section_y += c.tab_row * body;
                            }
                            frame.shapes.push(Shape {
                                group: line_group,
                                x: column_x,
                                y: top,
                                width: c.tab_rule * body,
                                height: section_y - top,
                                radius: 0.0,
                                stroke: None,
                                ink: Ink::Faint,
                            });
                            line_group += 1;
                        }
                        continue;
                    }
                    let visible = o.visible_lines(section);
                    for line in section.lines.iter().take(visible) {
                        let rows = o.rows(section, line, &m, wrap);
                        let row_height = o.row_height(section, line, &m) * body;
                        for (row_index, row) in rows.iter().enumerate() {
                            if row_index > 0 {
                                section_y += row_height;
                            }
                            for run in &row.runs {
                                if line.chord_only
                                    && run.role == Role::Lyric
                                    && run.text.trim().is_empty()
                                {
                                    continue;
                                }
                                let style = metrics.style(run.role);
                                let (baseline, ink) = match run.role {
                                    Role::Chord if line.chord_only => {
                                        (c.inline_lyric_baseline, Ink::Soft)
                                    }
                                    Role::Chord => (c.chord_baseline, Ink::Soft),
                                    Role::InlineChord => (c.inline_chord_baseline, Ink::Soft),
                                    Role::Lyric if inline => (c.inline_lyric_baseline, Ink::Strong),
                                    Role::Comment if inline => (c.inline_lyric_baseline, Ink::Soft),
                                    Role::Comment => (c.lyric_baseline, Ink::Soft),
                                    _ => (c.lyric_baseline, Ink::Strong),
                                };
                                frame.text.push(Text {
                                    group: line_group,
                                    content: run.text.clone(),
                                    x: column_x + run.x * body,
                                    baseline: section_y + baseline * body,
                                    size: style.size * body,
                                    weight: style.weight,
                                    face: style.face,
                                    tracking: style.tracking,
                                    ink,
                                });
                            }
                        }
                        section_y += row_height;
                        line_group += 1;
                    }
                    if visible == 0 {
                        section_y += o.label_row(section, &m) * body;
                        line_group += 1;
                    }
                }
                let column_width = solution.blocks[range.clone()]
                    .iter()
                    .map(|b| b.width)
                    .fold(0.0, f32::max);
                column_x += (column_width + c.column_gutter) * body;
            }
        } else {
            frame.text.push(Text {
                group: usize::MAX,
                content: "Song needs paging at this readable size".into(),
                x: c.margin_x,
                baseline: (y + body).min(content_end),
                size: body,
                weight: 600,
                face: Face::System,
                tracking: 0.0,
                ink: Ink::Soft,
            });
        }
        let status = if frame.fits && width < 500.0 {
            format!(
                "FIT{}  ·  {body} pt  ·  step {}",
                self.key_status("  ·  ", false),
                frame.ladder_step
            )
        } else if frame.fits {
            format!(
                "FIT{}{STATUS_SEPARATOR}{} {}{STATUS_SEPARATOR}stand {} pt",
                self.key_status(STATUS_SEPARATOR, true),
                frame.columns,
                if frame.columns == 1 {
                    "column"
                } else {
                    "columns"
                },
                body
            ) + &if frame.ladder_step > 0 {
                format!("{STATUS_SEPARATOR}compact {}", frame.ladder_step)
            } else {
                String::new()
            }
        } else {
            format!("PAGE NEEDED{STATUS_SEPARATOR}stand {} pt", body)
        };
        let status = self.status_texts(width, height, body, status, Hints::Fit);
        frame.text.extend(status);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, text: &str, size_pt: f32, _: u16, _: Face) -> f32 {
            text.chars().count() as f32 * size_pt * 0.5
        }
    }

    fn app(source: &str) -> App {
        App::new(
            cpl_song::parse(source).song,
            Config::default(),
            Polarity::Light,
            &Fixed,
        )
    }

    /// The tallest window at this width whose page needs exactly `step`.
    /// Searching heights keeps these tests independent of the vertical em table.
    fn at_step(app: &App, width: f32, step: u8) -> Frame {
        (40..1600)
            .rev()
            .map(|height| app.frame(width, height as f32))
            .find(|frame| frame.fits && frame.ladder_step == step)
            .unwrap_or_else(|| panic!("no height at width {width} needs step {step}"))
    }

    /// The first drawn item with this text, preferring the song body: a
    /// chord also names a diagram in the header (group 0).
    fn find<'a>(frame: &'a Frame, content: &str) -> &'a Text {
        let matching = || frame.text.iter().filter(|item| item.content == content);
        matching()
            .find(|item| item.group != 0)
            .or_else(|| matching().next())
            .unwrap_or_else(|| panic!("{content:?} not drawn"))
    }

    fn count(frame: &Frame, content: &str) -> usize {
        frame.text.iter().filter(|t| t.content == content).count()
    }

    #[test]
    fn song_chords_precede_their_lyrics() {
        let app =
            app("{title: Test}\n{start_of_verse: Verse 1}\n[C]Hello [G]world\n{end_of_verse}");
        let frame = app.frame(900.0, 600.0);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 0);
        let chord = find(&frame, "C");
        let lyric = find(&frame, "Hello ");
        assert!(chord.baseline < lyric.baseline);
        assert_eq!(chord.x, lyric.x);
    }

    #[test]
    fn never_draws_overflowing_lyrics_under_the_floor() {
        let app = app("{title: Test}\n[C]Hello");
        let frame = app.frame(100.0, 90.0);
        assert!(!frame.fits);
        assert!(!frame.text.iter().any(|t| t.content == "Hello"));
    }

    #[test]
    fn larger_type_is_checked_with_its_actual_font_width() {
        struct WiderAtLargeSizes;
        impl Measure for WiderAtLargeSizes {
            fn width(&self, text: &str, size: f32, _: u16, _: Face) -> f32 {
                text.chars().count() as f32 * size * 0.5 * if size > 17.0 { 1.5 } else { 1.0 }
            }
        }
        let song = cpl_song::parse("Hello world").song;
        let app = App::new(song, Config::default(), Polarity::Light, &WiderAtLargeSizes);
        let frame = app.frame(260.0, 600.0);
        let lyric = find(&frame, "Hello world");
        assert!(frame.fits);
        assert!(lyric.size > 17.0 && lyric.size < 28.0);
        let width = WiderAtLargeSizes.width(&lyric.content, lyric.size, lyric.weight, lyric.face);
        assert!(lyric.x + width <= frame.width - app.config.margin_x);
    }

    #[test]
    fn roomy_page_uses_the_configured_maximum() {
        let frame = app("Hello").frame(900.0, 600.0);
        assert!(frame.fits);
        assert_eq!(find(&frame, "Hello").size, 28.0);
    }

    #[test]
    fn sections_flow_into_columns_without_shrinking_lyrics() {
        let app = app(
            "{title: Test}\n{start_of_verse: One}\n[C]Alpha\n{end_of_verse}\n{start_of_chorus: Two}\n[G]Bravo\n{end_of_chorus}\n{start_of_verse: Three}\n[Am]Charlie\n{end_of_verse}",
        );
        let frame = (40..1600)
            .rev()
            .map(|h| app.frame(900.0, h as f32))
            .find(|f| f.fits && f.columns == 3 && f.ladder_step == 0)
            .expect("a height where three columns fit the full page");
        let lyrics: Vec<_> = ["Alpha", "Bravo", "Charlie"]
            .iter()
            .map(|l| find(&frame, l))
            .collect();
        assert!(lyrics.windows(2).all(|pair| pair[0].x < pair[1].x));
        assert!(lyrics.iter().all(|item| item.size >= 17.0));
        assert!(lyrics.iter().all(|item| item.size == lyrics[0].size));
    }

    #[test]
    fn removes_section_gap_before_anything_else() {
        let app = app(
            "{title: Test}\n{start_of_verse: Verse 1}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: Verse 2}\n[G]Bravo\n{end_of_verse}",
        );
        let frame = at_step(&app, 190.0, 1);
        assert_eq!(frame.columns, 1);
        let body = find(&frame, "Bravo").size;
        assert!(body >= 17.0);
        assert_eq!(find(&frame, "Test").size, body * app.config().title_size);
    }

    #[test]
    fn moves_short_labels_to_side_after_spacing_compacts() {
        let app = app(
            "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: B}\n[G]Bravo\n{end_of_verse}",
        );
        let frame = at_step(&app, 190.0, 2);
        let label = find(&frame, "A");
        let lyric = find(&frame, "Alpha");
        assert!(label.x < lyric.x);
        assert_eq!(label.baseline, lyric.baseline);
    }

    #[test]
    fn the_byline_keeps_its_own_line_in_any_window() {
        // Step 3 (title and byline on one line) is gone: a wide window used
        // to collapse the header that a narrower one kept.
        let app = app(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: B}\n[G]Bravo\n{end_of_verse}",
        );
        for width in [190.0, 600.0, 1900.0] {
            for height in (40..1200).step_by(7) {
                let frame = app.frame(width, height as f32);
                assert_ne!(frame.ladder_step, 3, "{width}×{height}");
                let title = frame.text.iter().find(|t| t.content == "Test");
                let byline = frame.text.iter().find(|t| t.content == "Ada");
                if let (Some(title), Some(byline)) = (title, byline) {
                    assert!(byline.baseline > title.baseline, "{width}×{height}");
                }
            }
        }
    }

    #[test]
    fn long_labels_skip_side_margin() {
        let app = app(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: Verse One}\n[C]Alpha\n{end_of_verse}",
        );
        assert!((40..1600).all(|h| app.frame(190.0, h as f32).ladder_step != 2));
        let frame = at_step(&app, 190.0, 7);
        assert_eq!(find(&frame, "VERSE ONE").x, find(&frame, "Alpha").x);
    }

    #[test]
    fn tablature_draws_small_and_soft_beside_a_rule_and_stays_in_the_source() {
        let app = app(
            "{title: Test}\n{start_of_tab: Intro}\ne|--3---|\nB|--1---|\n{end_of_tab}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}",
        );
        assert_eq!(app.song().sections[0].kind, SectionKind::Tab);
        assert_eq!(app.song().sections[0].lines[0].lyrics(), "e|--3---|");
        // Tall and narrow, so the tab and the verse share one column.
        let frame = app.frame(500.0, 900.0);
        assert!(frame.fits);
        assert_eq!(frame.columns, 1);
        assert_eq!(count(&frame, "INTRO"), 1);
        let (e, b, alpha) = (
            find(&frame, "e|--3---|"),
            find(&frame, "B|--1---|"),
            find(&frame, "Alpha"),
        );
        assert_eq!(e.face, Face::Monospace);
        assert_eq!(e.ink, Ink::Soft);
        assert!(
            e.size < alpha.size,
            "tab {} under lyric {}",
            e.size,
            alpha.size
        );
        assert_eq!(e.group, b.group, "a staff is one group");
        assert!(e.baseline < b.baseline && b.baseline < alpha.baseline);
        let rule = frame
            .shapes
            .iter()
            .find(|shape| shape.group == e.group)
            .expect("a rule beside the staff");
        assert_eq!(rule.ink, Ink::Faint);
        assert!(rule.x < e.x);
        assert!(rule.y < e.baseline - e.size * 0.7 && rule.y + rule.height > b.baseline);
        assert!(!app.tabs_folded(900.0, 600.0));
    }

    #[test]
    fn a_wide_staff_breaks_into_systems_that_scroll_and_page_whole() {
        let mut source = String::from("{title: Test}\n{start_of_tab: Riff}\n");
        for string in ["e", "B", "G", "D", "A", "E"] {
            source.push_str(string);
            source.push('|');
            for _ in 0..8 {
                source.push_str("--3--5--7--|");
            }
            source.push('\n');
        }
        source.push_str("{end_of_tab}\n{start_of_verse: A}\n");
        for index in 0..12 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{end_of_verse}");
        let mut app = app(&source);
        app.set_tabs(Tabs::Show);
        let (width, height) = (420.0, 300.0);
        let page = app.page(width, height);
        let rows: Vec<&Text> = page
            .text
            .iter()
            .filter(|item| item.content.starts_with("e|"))
            .collect();
        assert!(rows.len() > 1, "the staff wraps");
        for row in &rows {
            assert!(
                row.x + row.content.len() as f32 * row.size * 0.5 <= width - app.config().margin_x
            );
        }
        // Each system's six rows share one group, so a view shows all of a
        // system or none of it.
        for offset in [0.0, 40.0, 90.0, 140.0, 200.0] {
            let (view, _) = app.frame_scrolled(width, height, offset);
            let staff_row = |i: &Text| i.content.chars().nth(1) == Some('|');
            for item in view.text.iter().filter(|i| i.content.starts_with("e|")) {
                let group = item.group;
                let shown = view
                    .text
                    .iter()
                    .filter(|i| i.group == group && staff_row(i))
                    .count();
                assert_eq!(shown, 6, "offset {offset}: system cut");
            }
        }
        for page in 0..app.page_count(width, height) {
            let view = app.page_view(width, height, page);
            let (top, bottom) = view.band;
            let shown: Vec<Text> = page_items(&app, width, height, view.offset, top, bottom);
            for item in shown.iter().filter(|i| i.content.starts_with("e|")) {
                let rows = shown
                    .iter()
                    .filter(|i| i.group == item.group && i.content.chars().nth(1) == Some('|'));
                assert_eq!(rows.count(), 6);
            }
        }
    }

    fn page_items(
        app: &App,
        width: f32,
        height: f32,
        offset: f32,
        top: f32,
        bottom: f32,
    ) -> Vec<Text> {
        app.page(width, height)
            .text
            .into_iter()
            .filter(|item| {
                let baseline = item.baseline - offset;
                baseline - item.size >= top - 1e-3 && baseline <= bottom + 1e-3
            })
            .collect()
    }

    #[test]
    fn the_viewer_pins_tabs_shown_or_folded() {
        // A staff too wide to sit in a column beside the verse.
        let mut source = String::from("{title: Test}\n{start_of_tab: Intro}\n");
        for string in ["e", "B", "G", "D", "A", "E"] {
            source.push_str(&format!(
                "{string}|--3---|--1---|--0---|--2---|--3---|--0---|\n"
            ));
        }
        source.push_str("{end_of_tab}\n{start_of_verse: A}\n");
        // Lines too long for two columns, so folding the tab is what makes room.
        for index in 0..6 {
            source.push_str(&format!("[C]Line {index} of a long and winding verse\n"));
        }
        source.push_str("{end_of_verse}");
        // At the 17 pt floor, where shrinking cannot make the room.
        let mut app = App::new(
            cpl_song::parse(&source).song,
            Config::default().with_floor(17.0).unwrap(),
            Polarity::Light,
            &Fixed,
        );
        let width = 600.0;
        let auto = at_step(&app, width, 5);
        let height = auto.height;
        assert!(app.tabs_folded(width, height));
        let staff = |frame: &Frame| {
            frame
                .text
                .iter()
                .filter(|item| item.content.starts_with("e|"))
                .count()
        };
        assert_eq!(staff(&auto), 0);
        assert_eq!(count(&auto, "INTRO"), 1);
        // Beside the label, whether that sits above the section or in the
        // margin (step 2 runs before step 5).
        let cue = auto
            .text
            .iter()
            .find(|item| item.content.ends_with("FOLDED"))
            .expect("a folded tab says so");
        assert_eq!(cue.ink, Ink::Faint);
        assert_eq!(cue.baseline, find(&auto, "INTRO").baseline);
        assert!(cue.x > find(&auto, "INTRO").x);
        app.set_tabs(Tabs::Show);
        assert!(!app.tabs_folded(width, height));
        assert!(
            app.page(width, height)
                .text
                .iter()
                .all(|item| !item.content.ends_with("FOLDED"))
        );
        assert_eq!(staff(&app.page(width, height)), 1);
        app.set_tabs(Tabs::Fold);
        assert!(app.tabs_folded(1400.0, 1400.0));
        assert_eq!(staff(&app.frame(1400.0, 1400.0)), 0);
    }

    #[test]
    fn title_drops_only_after_earlier_candidates_fail() {
        let app = app("{title: Test}\n{start_of_verse: Verse One}\n[C]Alpha\n{end_of_verse}");
        let frame = at_step(&app, 190.0, 7);
        assert_eq!(count(&frame, "Test"), 0);
        assert!(find(&frame, "Alpha").size >= 17.0);
    }

    #[test]
    fn repeat_chorus_keeps_its_first_line_at_step_eight() {
        let app = app(
            "{title: Test}\n{start_of_chorus: A}\n[C]Echo\n[G]Again\n{end_of_chorus}\n{start_of_chorus: A}\n[C]Echo\n[G]Again\n{end_of_chorus}",
        );
        let frame = at_step(&app, 150.0, 8);
        assert_eq!(count(&frame, "Echo"), 2);
        assert_eq!(count(&frame, "Again"), 1);
    }

    #[test]
    fn inline_chords_are_last_fit_that_keeps_lyrics_readable() {
        let app = app(
            "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n[G]Bravo\n[Am]Charlie\n{end_of_verse}",
        );
        let frame = at_step(&app, 300.0, 9);
        let chord = find(&frame, "C");
        let lyric = find(&frame, "Alpha");
        assert!(chord.baseline < lyric.baseline);
        assert!(chord.x < lyric.x);
        assert!(lyric.size >= 17.0);
    }

    #[test]
    fn tablature_scrolls_with_the_song() {
        let mut source = String::from(
            "{title: Test}\n{start_of_tab: Tab}\ne|--3---|\n{end_of_tab}\n{start_of_verse: A}\n",
        );
        for index in 0..18 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{end_of_verse}");
        let app = app(&source);
        let (first, max_offset) = app.frame_scrolled(450.0, 260.0, 0.0);
        assert!(max_offset > 0.0);
        assert_eq!(count(&first, "TAB"), 1);
        assert_eq!(count(&first, "e|--3---|"), 1);
        let (last, _) = app.frame_scrolled(450.0, 260.0, max_offset);
        assert_eq!(count(&last, "e|--3---|"), 0);
        assert_eq!(count(&last, "Line 17"), 1);
    }

    #[test]
    fn first_scroll_view_shows_the_intro_after_a_tab() {
        let mut source = String::from(
            "{title: Test}\n{start_of_tab: Tab}\ne|--3---|\n{end_of_tab}\n{start_of_verse: Intro}\n[G] [Am] [C] [G]\n{end_of_verse}\n{start_of_verse: Verse 1}\n",
        );
        for index in 0..18 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{end_of_verse}");
        let app = app(&source);
        let (first, max_offset) = app.frame_scrolled(450.0, 300.0, 0.0);
        assert!(max_offset > 0.0);
        assert_eq!(count(&first, "e|--3---|"), 1);
        assert_eq!(count(&first, "INTRO"), 1);
        assert_eq!(count(&first, "Am"), 1);
        assert!(!first.text.iter().any(|item| item.content.trim().is_empty()));
    }

    #[test]
    fn plain_text_after_a_tab_remains_visible() {
        let app = app(
            "{title: Test}\n{start_of_tab}\ne|--3--|\n{end_of_tab}\nSlide up on repeat\n{start_of_verse: Verse 1}\n[C]Hello\n{end_of_verse}",
        );
        let frame = app.frame(900.0, 600.0);
        assert_eq!(count(&frame, "e|--3--|"), 1);
        assert_eq!(count(&frame, "Slide up on repeat"), 1);
    }

    #[test]
    fn scroll_status_keeps_the_section_name_when_its_label_has_left_view() {
        let mut source = String::from(
            "{title: Test}\n{sov: Verse 1}\n[C]First\n[G]Second\n{eov}\n{soc: Chorus}\n[F]Echo\n[C]Again\n{eoc}\n{sov: Verse 2}\n",
        );
        for index in 0..18 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{eov}");
        let app = app(&source);
        let (_, max_offset) = app.frame_scrolled(450.0, 260.0, 0.0);
        let frame = (0..=max_offset.ceil() as usize)
            .step_by(10)
            .map(|offset| app.frame_scrolled(450.0, 260.0, offset as f32).0)
            .find(|frame| count(frame, "Again") == 1 && count(frame, "CHORUS") == 0)
            .expect("the second chorus line can be visible after its label scrolls away");
        assert!(
            frame
                .text
                .iter()
                .any(|item| item.group == usize::MAX
                    && item.content.starts_with("CHORUS  ·  SCROLL"))
        );
    }

    #[test]
    fn pages_turn_on_whole_groups_without_repeating_or_skipping_a_line() {
        let mut source = String::from("{title: Test}\n{sov: Verse 1}\n");
        for index in 0..25 {
            source.push_str(&format!("[C]Line {index} [G]here\n"));
        }
        source.push_str("{eov}");
        let app = app(&source);
        let (width, height) = (450.0, 300.0);
        let count = app.page_count(width, height);
        assert!(count >= 3, "{count}");
        let page = app.page(width, height);
        let mut seen = Vec::new();
        for index in 0..count {
            let view = app.page_view(width, height, index);
            let status = view.status.first().unwrap();
            assert!(
                status
                    .content
                    .contains(&format!("PAGE {} / {count}", index + 1)),
                "{}",
                status.content
            );
            for item in page.text.iter().filter(|t| t.content.starts_with("Line ")) {
                let y = item.baseline - view.offset;
                let top = y - item.size;
                if top >= view.band.0 && y <= view.band.1 {
                    seen.push(item.content.clone());
                }
            }
        }
        let expected: Vec<String> = (0..25).map(|i| format!("Line {i} ")).collect();
        assert_eq!(seen, expected, "every line once, in order");
        // The page past the end clamps to the last.
        assert_eq!(
            app.page_view(width, height, 99).offset,
            app.page_view(width, height, count - 1).offset
        );
    }

    #[test]
    fn a_fitted_song_is_one_page() {
        let app = app("{title: Test}\n{sov: A}\n[C]Hello\n{eov}");
        assert_eq!(app.page_count(900.0, 600.0), 1);
        assert!(app.page_view(900.0, 600.0, 3).status.is_empty());
    }

    #[test]
    fn a_narrow_scroll_status_fits_its_window() {
        let mut source = String::from("{title: T}\n{sov: Verse 1}\n");
        for index in 0..20 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{eov}");
        let app = app(&source);
        let width = 280.0;
        let (frame, max_offset) = app.frame_scrolled(width, 420.0, 0.0);
        assert!(max_offset > 0.0);
        let status = frame.text.iter().find(|t| t.group == usize::MAX).unwrap();
        assert_eq!(status.content, "VERSE 1  ·  SCROLL  ·  ↓");
        // Fixed measures half an em per character; the monospaced face is
        // wider in practice, so leave it a 20% margin.
        let drawn = status.content.chars().count() as f32 * status.size * 0.5 * 1.2;
        assert!(status.x + drawn <= width - app.config().margin_x, "{drawn}");
    }

    #[test]
    fn the_status_names_the_key_and_any_live_transpose() {
        let status = |app: &App, width: f32| {
            app.frame(width, 600.0)
                .text
                .into_iter()
                .find(|t| t.group == usize::MAX)
                .unwrap()
                .content
        };
        let source = "{title: T}\n{key: Am}\n{sov: A}\n[Am]Hello\n{eov}";
        let plain = app(source);
        assert!(
            status(&plain, 1000.0).starts_with("FIT   ·   Am   ·   1 column"),
            "{}",
            status(&plain, 1000.0)
        );
        assert!(
            status(&plain, 400.0).starts_with("FIT  ·  28 pt"),
            "narrow: key only when shifted: {}",
            status(&plain, 400.0)
        );
        let song = cpl_song::parse(source).song.transposed(2);
        let up = App::new(song, Config::default(), Polarity::Light, &Fixed);
        assert!(status(&up, 1000.0).starts_with("FIT   ·   Bm +2   ·   "));
        assert!(status(&up, 400.0).starts_with("FIT  ·  Bm +2  ·  28 pt"));
        let keyless = cpl_song::parse("{title: T}\n[C]Hi").song.transposed(-3);
        let keyless = App::new(keyless, Config::default(), Polarity::Light, &Fixed);
        assert!(status(&keyless, 1000.0).starts_with("FIT   ·   transpose −3   ·   "));
    }

    #[test]
    fn comments_draw_in_the_soft_ink_on_the_lyric_baseline() {
        let app = app("{title: Test}\n{sov: A}\n[C]Alpha\n{c: Repeat x2}\n{eov}");
        let frame = app.frame(600.0, 600.0);
        let comment = find(&frame, "Repeat x2");
        let lyric = find(&frame, "Alpha");
        assert_eq!(comment.ink, Ink::Soft);
        assert_eq!(comment.size, lyric.size);
        assert_eq!(comment.x, lyric.x);
        let row = app.config().line_height * lyric.size;
        assert!((comment.baseline - lyric.baseline - row).abs() < 0.01);
    }

    #[test]
    fn chord_only_intro_uses_a_compact_row_before_the_lyric() {
        let app = app("{title: Test}\n{sov: Intro}\n[G] [Am] [C] [G]\n[C]Hello\n{eov}");
        let frame = app.frame(900.0, 600.0);
        let g = find(&frame, "G");
        let am = find(&frame, "Am");
        let lyric = find(&frame, "Hello");
        let body = lyric.size;
        assert_eq!(g.baseline, am.baseline);
        assert_eq!(g.size, app.config.chord_size * body);
        assert!(!frame.text.iter().any(|item| item.content == " "));
        let expected = (app.config.inline_line_height + app.config.lyric_baseline
            - app.config.inline_lyric_baseline)
            * body;
        assert!((lyric.baseline - g.baseline - expected).abs() < 0.01);
    }

    #[test]
    fn a_recalled_chorus_collapses_at_step_eight() {
        let app = app("{title: Test}\n{soc: A}\n[C]Echo\n[G]Again\n{eoc}\n{chorus}");
        let frame = at_step(&app, 150.0, 8);
        assert_eq!(count(&frame, "Echo"), 2);
        assert_eq!(count(&frame, "Again"), 1);
    }

    #[test]
    fn blank_line_stanzas_flow_into_columns() {
        let mut source = String::from("{title: Test}\n");
        for verse in 0..4 {
            for line in 0..4 {
                source.push_str(&format!("[C]verse {verse} line {line}\n"));
            }
            source.push('\n');
        }
        let frame = app(&source).frame(1200.0, 400.0);
        assert!(frame.fits);
        assert!(frame.columns >= 2);
    }

    #[test]
    fn a_song_with_no_sections_fits() {
        let frame = app("{title: Only a title}").frame(600.0, 400.0);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 0);
    }

    #[test]
    fn a_scroll_view_bands_exactly_the_groups_frame_scrolled_would_draw() {
        let mut source = String::from("{title: Test}\n{start_of_verse: Verse 1}\n");
        for index in 0..10 {
            source.push_str(&format!("[C]Line {index} [G]here\n"));
        }
        source.push_str("{end_of_verse}\n{start_of_chorus: Chorus}\n");
        for index in 0..8 {
            source.push_str(&format!("[F]Chorus {index}\n"));
        }
        source.push_str("{end_of_chorus}");
        let app = app(&source);
        let (width, height) = (400.0, 300.0);
        let page = app.page(width, height);
        assert!(!page.fits, "this song overflows at the floor");
        assert!(page.height > height);
        assert!(
            page.text.iter().all(|t| t.group != usize::MAX),
            "no status in the page"
        );
        for offset in [0.0, 37.0, 150.0, 1e9] {
            let (frame, max_offset) = app.frame_scrolled(width, height, offset);
            let view = app.view(width, height, offset);
            assert_eq!(view.max_offset, max_offset);
            assert_eq!(view.offset, offset.min(max_offset));
            let status = view.status.first().expect("scroll status");
            let drawn_status = frame.text.iter().find(|t| t.group == usize::MAX).unwrap();
            assert_eq!(status.content, drawn_status.content);
            // Every drawn song item sits inside the band; every page item the
            // frame withheld lies outside it.
            let visible: Vec<&Text> = frame
                .text
                .iter()
                .filter(|t| t.group != usize::MAX)
                .collect();
            for item in &visible {
                assert!(item.baseline - item.size >= view.band.0 - 1e-3);
                assert!(item.baseline <= view.band.1 + 1e-3);
            }
            for item in page.text.iter() {
                let shown = visible.iter().any(|v| {
                    v.content == item.content
                        && (v.baseline + view.offset - item.baseline).abs() < 1e-3
                });
                if !shown {
                    let window_baseline = item.baseline - view.offset;
                    assert!(
                        window_baseline - item.size * 0.9 >= view.band.1
                            || window_baseline + item.size * 0.25 <= view.band.0,
                        "{:?} at {window_baseline} overlaps band {:?} at offset {offset}",
                        item.content,
                        view.band
                    );
                }
            }
        }
    }

    #[test]
    fn a_wide_lyric_line_wraps_instead_of_asking_for_width() {
        // Fixed: 0.5 em per character at 17 pt, so this line is ~290 pt wide.
        let app = app(
            "{title: T}\n{sov: Verse 1}\n[C]Hello there my [G]friend of old, we [Am]meet again\n{eov}",
        );
        // 240 pt less the margins is 168 pt: 9.9 em, so "friend " cannot
        // follow "Hello there my " (7.5 em) on the first row.
        let frame = app.frame(240.0, 600.0);
        assert!(frame.fits, "the song wraps and fits");
        assert!(!frame.text.iter().any(|t| t.content.starts_with("WIDTH")));
        let hello = find(&frame, "Hello there my ");
        let meet = frame
            .text
            .iter()
            .find(|t| t.content.starts_with("meet"))
            .expect("the tail of the line is drawn");
        assert!(
            meet.baseline > hello.baseline,
            "the tail sits on a later row"
        );
        assert!(meet.x > hello.x, "continuation rows are indented");
        let am = find(&frame, "Am");
        assert_eq!(am.x, meet.x, "the chord follows its word onto the new row");
        assert_eq!(am.group, hello.group, "one source line is one group");
        let right_edge = 240.0 - app.config().margin_x + 1e-2;
        for item in frame.text.iter().filter(|t| t.group != usize::MAX) {
            let width = item.content.chars().count() as f32 * item.size * 0.5;
            assert!(
                item.x + width <= right_edge,
                "{item:?} runs past the content box"
            );
        }
    }

    #[test]
    fn only_an_unbreakable_run_still_needs_width() {
        let app =
            app("{title: T}\n{sov: A}\n[C]Antidisestablishmentarianismandthensomemore\n{eov}");
        let (frame, max_offset) = app.frame_scrolled(220.0, 400.0, 0.0);
        assert!(!frame.fits);
        assert_eq!(max_offset, 0.0);
        assert!(
            frame
                .text
                .iter()
                .any(|t| t.content.starts_with("WIDTH NEEDED")),
            "{:?}",
            frame.text.iter().map(|t| &t.content).collect::<Vec<_>>()
        );
        // A scrolling page with a wide title still shows the song.
        let mut source =
            String::from("{title: A Title Far Too Wide For This Narrow Window Indeed}\n{sov: A}\n");
        for index in 0..30 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{eov}");
        let wide_title = App::new(
            cpl_song::parse(&source).song,
            Config::default().with_floor(17.0).unwrap(),
            Polarity::Light,
            &Fixed,
        );
        // In a short window the wrapped title would fill the viewport, so it
        // drops like ladder step 7 and the song starts on the first screen.
        let (frame, max_offset) = wide_title.frame_scrolled(220.0, 300.0, 0.0);
        assert!(max_offset > 0.0);
        assert_eq!(count(&frame, "Line 0"), 1);
        assert!(!frame.text.iter().any(|t| t.content.starts_with("A Title")));
        // In a tall one it wraps inside the content box and stays.
        let (tall, tall_max) = wide_title.frame_scrolled(300.0, 900.0, 0.0);
        assert!(tall_max > 0.0, "still a scrolling page");
        let right_edge = 300.0 - wide_title.config().margin_x + 1e-2;
        let title_rows: Vec<&Text> = tall
            .text
            .iter()
            .filter(|t| t.weight == 700 && t.group == 0)
            .collect();
        assert!(title_rows.len() > 1, "the title wraps");
        for row in &title_rows {
            let width = row.content.chars().count() as f32 * row.size * 0.5;
            assert!(
                row.x + width <= right_edge,
                "{:?} crosses the margin",
                row.content
            );
        }
        let (end, _) = wide_title.frame_scrolled(220.0, 300.0, max_offset);
        assert_eq!(count(&end, "Line 29"), 1, "the whole song is reachable");
    }

    #[test]
    fn a_roomy_page_draws_a_diagram_per_chord_using_the_songs_defines() {
        let app = app(
            "{title: T}\n{define: Am base-fret 5 frets x 0 2 2 1 0}\n{sov: A}\n[Am]One [C]two\n{eov}",
        );
        let frame = app.frame(900.0, 700.0);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 0);
        let names: Vec<&str> = frame
            .text
            .iter()
            .filter(|t| t.group == 0 && (t.content == "Am" || t.content == "C"))
            .map(|t| t.content.as_str())
            .collect();
        assert_eq!(names, ["Am", "C"], "diagram names in first-use order");
        assert_eq!(count(&frame, "5fr"), 1, "the define's base fret is noted");
        assert!(
            frame
                .shapes
                .iter()
                .any(|s| s.group == 0 && s.stroke.is_none())
        );
        // The tallest window that needs step 4 has no diagram shapes.
        let without = at_step(&app, 900.0, 4);
        assert_eq!(count(&without, "5fr"), 0);
        // Only the header rule is left: no grid lines, nut or dots.
        let rule = app.config().header_rule;
        assert!(
            without
                .shapes
                .iter()
                .all(|s| s.stroke.is_none() && s.radius == 0.0 && s.height <= rule * 17.0 * 2.0)
        );
        assert_eq!(without.shapes.len(), 1);
    }

    #[test]
    fn a_fitted_page_views_as_itself() {
        let app =
            app("{title: Test}\n{start_of_verse: Verse 1}\n[C]Hello [G]world\n{end_of_verse}");
        let page = app.page(900.0, 600.0);
        assert!(page.fits);
        assert!(
            page.text.iter().any(|t| t.group == usize::MAX),
            "a fitted page keeps its status"
        );
        let view = app.view(900.0, 600.0, 500.0);
        assert_eq!(view.offset, 0.0);
        assert_eq!(view.band, (0.0, 600.0));
        assert!(view.status.is_empty());
    }

    #[test]
    fn overflow_scrolls_to_the_last_lyric_at_the_readability_floor() {
        let mut source = String::from("{title: Test}\n{start_of_verse: A}\n");
        for index in 0..18 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{end_of_verse}");
        let app = app(&source);
        let (first, max_offset) = app.frame_scrolled(450.0, 260.0, 0.0);
        assert_eq!(first.ladder_step, 10);
        assert!(max_offset > 0.0);
        assert_eq!(count(&first, "Line 0"), 1);
        assert_eq!(count(&first, "Line 17"), 0);
        let prepared = {
            let cache = app.prepared.borrow();
            Arc::clone(&cache.as_ref().unwrap().2)
        };
        for chord in first.text.iter().filter(|item| item.content == "C") {
            assert!(
                first
                    .text
                    .iter()
                    .any(|item| item.group == chord.group && item.content.starts_with("Line "))
            );
        }
        let (last, _) = app.frame_scrolled(450.0, 260.0, max_offset);
        let reused = {
            let cache = app.prepared.borrow();
            Arc::clone(&cache.as_ref().unwrap().2)
        };
        assert!(Arc::ptr_eq(&prepared, &reused));
        assert_eq!(count(&last, "Line 17"), 1);
        assert!(
            last.text
                .iter()
                .filter(|item| item.content.starts_with("Line "))
                .all(|item| item.size == 17.0)
        );
    }

    #[test]
    fn default_config_is_complete() {
        assert!(Config::parse(include_str!("../../../config/default.toml")).is_ok());
        assert!(Config::parse("[page]\nmargin_x = 2\nunknown = 3").is_err());
    }

    #[test]
    fn a_user_config_overrides_only_the_keys_it_names() {
        let config =
            Config::with_overrides("[page]\nbody_size = 10 # floor\nmargin_x = 20\n").unwrap();
        let defaults = Config::default();
        assert_eq!(config.body_size, 10.0);
        assert_eq!(config.margin_x, 20.0);
        assert_eq!(config.max_body_size, defaults.max_body_size);
        assert_eq!(config.line_height, defaults.line_height);
        assert_eq!(
            Config::with_overrides("[page]\n").unwrap().body_size,
            defaults.body_size
        );
        assert!(
            Config::with_overrides("[page]\nbody_sise = 10\n")
                .unwrap_err()
                .contains("body_sise")
        );
    }

    #[test]
    fn a_floor_above_the_largest_size_raises_it() {
        let config = Config::with_overrides("[page]\nbody_size = 32\n").unwrap();
        assert_eq!(config.max_body_size, 32.0);
        assert!(Config::with_overrides("[page]\nbody_size = 32\nmax_body_size = 28\n").is_err());
        let floor = Config::default().with_floor(9.0).unwrap();
        assert_eq!(
            (floor.body_size, floor.max_body_size),
            (9.0, Config::default().max_body_size)
        );
        assert_eq!(
            Config::default().with_floor(40.0).unwrap().max_body_size,
            40.0
        );
        assert!(Config::default().with_floor(3.0).is_err());
    }

    #[test]
    fn text_shrinks_before_the_title_drops() {
        let mut source = String::from("{title: Test}\n{artist: Ada}\n");
        for verse in 0..3 {
            source.push_str(&format!("{{start_of_verse: V{verse}}}\n"));
            for line in 0..3 {
                source.push_str(&format!("[C]verse {verse} line {line}\n"));
            }
            source.push_str("{end_of_verse}\n");
        }
        let with = |config: Config| {
            App::new(
                cpl_song::parse(&source).song,
                config,
                Polarity::Light,
                &Fixed,
            )
        };
        let balanced = with(Config::default());
        let strict = with(Config::default().with_floor(17.0).unwrap());
        let title = |frame: &Frame| frame.text.iter().any(|t| t.content == "Test");
        // Somewhere the old ladder (floor 17, first fit wins) dropped the
        // title, the balanced page keeps it and sets the text smaller.
        let kept = (100..900).step_by(5).any(|height| {
            let height = height as f32;
            let old = strict.frame(300.0, height);
            let new = balanced.frame(300.0, height);
            old.fits && !title(&old) && new.fits && title(&new)
        });
        assert!(kept, "no height keeps the title by shrinking");
        // The title never drops while the text is still at the comfortable size.
        for height in (100..900).step_by(5) {
            let frame = balanced.frame(300.0, height as f32);
            if frame.fits && !title(&frame) {
                let lyric = frame
                    .text
                    .iter()
                    .find(|t| t.content.starts_with("verse"))
                    .unwrap();
                assert!(lyric.size < Config::default().comfort_size, "{height}");
            }
        }
    }

    #[test]
    fn a_page_that_cannot_fit_scrolls_at_the_comfortable_size() {
        let mut source = String::from("{title: Test}\n{sov: A}\n");
        for line in 0..60 {
            source.push_str(&format!("[C]Line {line}\n"));
        }
        source.push_str("{eov}");
        let app = app(&source);
        let (frame, max_offset) = app.frame_scrolled(500.0, 400.0, 0.0);
        assert!(max_offset > 0.0);
        let line = find(&frame, "Line 0");
        assert_eq!(line.size, Config::default().comfort_size);
    }

    #[test]
    fn a_lower_floor_fits_a_song_that_scrolled_at_the_default() {
        let mut source = String::from("{title: Test}\n");
        for verse in 0..6 {
            source.push_str(&format!("{{start_of_verse: V{verse}}}\n"));
            for line in 0..4 {
                source.push_str(&format!("[C]verse {verse} line {line}\n"));
            }
            source.push_str("{end_of_verse}\n");
        }
        let song = || cpl_song::parse(&source).song;
        let at =
            |config: Config| App::new(song(), config, Polarity::Light, &Fixed).frame(500.0, 400.0);
        assert!(!at(Config::default().with_floor(17.0).unwrap()).fits);
        assert!(at(Config::default().with_floor(8.0).unwrap()).fits);
    }
}
