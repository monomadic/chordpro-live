//! Headless song-page composition. No window or GPU types cross this boundary.

mod setlist;

pub use cpl_layout::{Face, Measure};
use cpl_layout::{MeasuredSong, Metrics, Options, PageMetrics, Role, Solution, Viewport};
use cpl_song::{SectionKind, Song};
pub use setlist::Setlist;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

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
    pub ink: Ink,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub polarity: Polarity,
    pub text: Vec<Text>,
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
    /// The status strip of a scrolling page. A fitted page draws its own.
    pub status: Option<Text>,
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
    pub body_size: f32,
    pub max_body_size: f32,
    pub header_gap: f32,
    pub section_gap: f32,
    pub line_height: f32,
    pub chord_baseline: f32,
    pub lyric_baseline: f32,
    pub label_height: f32,
    pub label_gap: f32,
    pub chord_gap: f32,
    pub inline_line_height: f32,
    pub inline_lyric_baseline: f32,
    pub inline_chord_baseline: f32,
    pub inline_chord_size: f32,
    pub inline_chord_gap: f32,
    pub column_gutter: f32,
    pub header_inline_gap: f32,
    pub byline_leading: f32,
    pub side_label_inset: f32,
    /// Indent of a wrapped line's continuation rows, in body sizes.
    pub wrap_indent: f32,
    pub status_height: f32,
    pub max_columns: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self::parse(include_str!("../../../config/default.toml"))
            .expect("bundled page config is valid")
    }
}

impl Config {
    /// Reads the numeric `[page]` table used by this initial view.
    pub fn parse(source: &str) -> Result<Self, String> {
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
            if values.insert(key, value).is_some() {
                return Err(format!("line {}: duplicate {key}", index + 1));
            }
        }
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
            max_body_size: take!("max_body_size"),
            header_gap: take!("header_gap"),
            section_gap: take!("section_gap"),
            line_height: take!("line_height"),
            chord_baseline: take!("chord_baseline"),
            lyric_baseline: take!("lyric_baseline"),
            label_height: take!("label_height"),
            label_gap: take!("label_gap"),
            chord_gap: take!("chord_gap"),
            inline_line_height: take!("inline_line_height"),
            inline_lyric_baseline: take!("inline_lyric_baseline"),
            inline_chord_baseline: take!("inline_chord_baseline"),
            inline_chord_size: take!("inline_chord_size"),
            inline_chord_gap: take!("inline_chord_gap"),
            column_gutter: take!("column_gutter"),
            header_inline_gap: take!("header_inline_gap"),
            byline_leading: take!("byline_leading"),
            side_label_inset: take!("side_label_inset"),
            wrap_indent: take!("wrap_indent"),
            status_height: take!("status_height"),
            max_columns: take!("max_columns"),
        };
        if let Some(key) = values.keys().next() {
            return Err(format!("unknown page key: {key}"));
        }
        if config.chord_baseline >= config.lyric_baseline
            || config.lyric_baseline >= config.line_height
            || config.inline_chord_baseline >= config.inline_lyric_baseline
            || config.inline_lyric_baseline >= config.inline_line_height
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
}

enum PreparedPage {
    Fit(Frame),
    NeedsWidth(Frame),
    Scroll {
        content: Vec<Text>,
        bounds: BTreeMap<usize, (f32, f32)>,
        sections: Vec<ScrollSection>,
        max_offset: f32,
    },
}

struct ScrollSection {
    groups: std::ops::Range<usize>,
    name: String,
}

impl App {
    /// Measures the floor and each whole-point candidate up to the configured
    /// maximum. Resizing, scrolling and polarity then need no host measurement.
    pub fn new(song: Song, config: Config, polarity: Polarity, measure: &impl Measure) -> Self {
        // Keep the parsed source intact. Tablature is temporarily omitted from
        // the performance page until it has a deliberate, readable layout.
        let display_song = Song {
            metadata: song.metadata.clone(),
            sections: song
                .sections
                .iter()
                .filter(|section| section.kind != SectionKind::Tab)
                .cloned()
                .collect(),
        };
        let measured = cpl_layout::measure_song(&display_song, &config.metrics(), measure);
        let mut larger = Vec::new();
        let mut size = config.body_size.floor() + 1.0;
        while size <= config.max_body_size {
            larger.push((
                size,
                cpl_layout::measure_song(&display_song, &config.metrics_at(size), measure),
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
                cpl_layout::measure_song(
                    &display_song,
                    &config.metrics_at(config.max_body_size),
                    measure,
                ),
            ));
        }
        Self {
            song,
            config,
            measured,
            larger,
            prepared: RefCell::new(None),
            polarity,
        }
    }

    pub fn song(&self) -> &Song {
        &self.song
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn page_metrics(&self, body: f32) -> PageMetrics {
        let c = &self.config;
        PageMetrics {
            title_size: c.title_size,
            subtitle_size: c.subtitle_size,
            byline_leading: c.byline_leading,
            header_gap: c.header_gap,
            header_inline_gap: c.header_inline_gap,
            section_gap: c.section_gap,
            label_height: c.label_height,
            label_gap: c.label_gap,
            line_height: c.line_height,
            inline_line_height: c.inline_line_height,
            column_gutter: c.column_gutter,
            side_label_room: c.margin_x.min(c.column_gutter * body) / body - c.side_label_inset,
            max_columns: c.max_columns as usize,
            wrap_indent: c.wrap_indent,
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
                max_offset,
                ..
            } => Frame {
                width,
                height: height + max_offset,
                polarity: self.polarity,
                text: content.clone(),
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
        match self.prepared(width, height).as_ref() {
            PreparedPage::Fit(_) | PreparedPage::NeedsWidth(_) => ScrollView {
                offset: 0.0,
                max_offset: 0.0,
                band: (0.0, height),
                status: None,
            },
            PreparedPage::Scroll {
                bounds,
                sections,
                max_offset,
                ..
            } => {
                let c = &self.config;
                let body = c.body_size;
                let offset = offset.clamp(0.0, *max_offset);
                let content_end = height - c.margin_bottom - c.status_height * body;
                let mut band = (f32::INFINITY, f32::NEG_INFINITY);
                let mut first_visible_group = None;
                for (&group, &(top, bottom)) in bounds {
                    if top - offset >= c.margin_top && bottom - offset <= content_end {
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
                let status = Text {
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
                    ink: Ink::Faint,
                };
                ScrollView {
                    offset,
                    max_offset: *max_offset,
                    band,
                    status: Some(status),
                }
            }
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
        let body = self.config.body_size;
        let more = if offset < max_offset {
            "  ·  MORE ↓"
        } else {
            ""
        };
        let section = first_visible_group.and_then(|group| {
            sections
                .iter()
                .find(|section| section.groups.contains(&group))
        });
        let section_prefix =
            section.map_or(String::new(), |section| format!("{}  ·  ", section.name));
        if width < 700.0 {
            format!("{section_prefix}SCROLL  ·  {body} pt{more}")
        } else {
            format!("{section_prefix}SCROLL  ·  stand {body} pt  ·  ↑ ↓ / Page Up Down{more}")
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
                bounds,
                sections,
                max_offset,
            } => {
                let c = &self.config;
                let body = c.body_size;
                let offset = offset.clamp(0.0, *max_offset);
                let content_end = height - c.margin_bottom - c.status_height * body;
                let mut frame = Frame {
                    width,
                    height,
                    polarity: self.polarity,
                    text: Vec::new(),
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
        let body = c.body_size;
        let metrics = self.page_metrics(body);
        // One unbounded column; the scroll viewport windows over it.
        let tall = Viewport {
            height: f32::INFINITY,
            ..self.viewport(width, height, body)
        };
        // Lines wrap at the content width, so only an unbreakable run (a tab
        // row, one enormous word) can still fail. A title wider than the
        // window clips at the edge rather than withholding the whole song.
        let mut long = cpl_layout::layout(&self.measured, &metrics, tall, Options::default(), 0);
        if !long.fit.meets_floor {
            let inline = Options {
                inline_chords: true,
                ..Options::default()
            };
            long = cpl_layout::layout(&self.measured, &metrics, tall, inline, 9);
        }
        // The scroll page tolerates a header wider than the window: it
        // clips at the edge instead of withholding the song.
        long.fits = long.fit.meets_floor;
        if !long.fits {
            for item in &mut fit.text {
                if item.content == "Song needs paging at this readable size" {
                    item.content = "Widen window to show this song".into();
                } else if item.content.starts_with("PAGE NEEDED") {
                    item.content = format!("WIDTH NEEDED  ·  stand {} pt", body);
                }
            }
            return PreparedPage::NeedsWidth(fit);
        }
        let mut long = self.compose(width, height, body, &self.measured, &long);
        long.text.pop(); // the long page's status strip belongs below its viewport
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
        let mut first_group = 1;
        let sections = self
            .measured
            .sections
            .iter()
            .map(|section| {
                let count = section.lines.len().max(1);
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
        PreparedPage::Scroll {
            content: long.text,
            bounds: group_bounds,
            sections,
            max_offset,
        }
    }

    /// The least-reduced page that fits, or the most-reduced candidate marked unfit.
    pub fn frame(&self, width: f32, height: f32) -> Frame {
        let floor = self.config.body_size;
        let solution = cpl_layout::solve(
            &self.measured,
            &self.page_metrics(floor),
            self.viewport(width, height, floor),
        );
        if solution.fits {
            for (body, measured) in self.larger.iter().rev() {
                let larger = cpl_layout::layout(
                    measured,
                    &self.page_metrics(*body),
                    self.viewport(width, height, *body),
                    solution.options,
                    solution.step,
                );
                // Bigger type is only better while it wraps no more lines
                // than the floor does: a line broken in three at 28 pt reads
                // worse than the same line whole at 20 pt.
                if larger.fits && larger.wrapped_rows <= solution.wrapped_rows {
                    return self.compose(width, height, *body, measured, &larger);
                }
            }
        }
        self.compose(width, height, floor, &self.measured, &solution)
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
            fits: solution.fits,
            columns: if solution.fits {
                solution.fit.columns.len()
            } else {
                0
            },
            ladder_step: solution.step,
        };
        if let Some(baseline) = solution.header.title_baseline {
            frame.text.push(Text {
                group: 0,
                content: header.title.clone(),
                x: c.margin_x,
                baseline: c.margin_top + baseline * body,
                size: c.title_size * body,
                weight: 600,
                face: Face::System,
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
                ink: Ink::Soft,
            });
        }
        let y = c.margin_top + solution.header.height * body;
        let content_end = height - c.margin_bottom - c.status_height * body;
        let wrap = self.viewport(width, height, body).width;
        if frame.fits {
            let metrics = c.metrics();
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
                            baseline: if o.side_labels {
                                section_y
                                    + (if section.lines.first().is_some_and(|line| line.chord_only)
                                        || inline
                                    {
                                        c.inline_lyric_baseline
                                    } else {
                                        c.lyric_baseline
                                    }) * body
                            } else {
                                section_y
                            },
                            size: c.label_size * body,
                            weight: if section.kind == SectionKind::Chorus {
                                700
                            } else {
                                600
                            },
                            face: Face::Monospace,
                            ink: Ink::Soft,
                        });
                        if !o.side_labels {
                            section_y += c.label_gap * body;
                        }
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
                ink: Ink::Soft,
            });
        }
        frame.text.push(Text {
            group: usize::MAX,
            content: if frame.fits && width < 500.0 {
                format!("FIT  ·  {body} pt  ·  step {}", frame.ladder_step)
            } else if frame.fits {
                format!(
                    "FIT  ·  {} {}  ·  stand {} pt",
                    frame.columns,
                    if frame.columns == 1 {
                        "column"
                    } else {
                        "columns"
                    },
                    body
                ) + &if frame.ladder_step > 0 {
                    format!("  ·  compact {}", frame.ladder_step)
                } else {
                    String::new()
                }
            } else {
                format!("PAGE NEEDED  ·  stand {} pt", body)
            },
            x: c.margin_x,
            baseline: height - c.margin_bottom * 0.5,
            size: body * 0.7,
            weight: 400,
            face: Face::Monospace,
            ink: Ink::Faint,
        });
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

    fn find<'a>(frame: &'a Frame, content: &str) -> &'a Text {
        frame
            .text
            .iter()
            .find(|item| item.content == content)
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
        assert_eq!(find(&frame, "Test").size, body * 1.7);
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
    fn collapses_header_after_side_labels() {
        let app = app(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: B}\n[G]Bravo\n{end_of_verse}",
        );
        let frame = at_step(&app, 190.0, 3);
        assert_eq!(find(&frame, "Test").baseline, find(&frame, "Ada").baseline);
    }

    #[test]
    fn long_labels_skip_side_margin_but_allow_header_collapse() {
        let app = app(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: Verse One}\n[C]Alpha\n{end_of_verse}",
        );
        assert!((40..1600).all(|h| app.frame(190.0, h as f32).ladder_step != 2));
        let frame = at_step(&app, 190.0, 3);
        assert_eq!(find(&frame, "VERSE ONE").x, find(&frame, "Alpha").x);
    }

    #[test]
    fn tablature_is_preserved_in_the_song_but_hidden_from_the_page() {
        let app = app(
            "{title: Test}\n{start_of_tab: Intro}\ne|--3---|\nB|--1---|\n{end_of_tab}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}",
        );
        assert_eq!(app.song().sections[0].kind, SectionKind::Tab);
        assert_eq!(app.song().sections[0].lines[0].lyrics(), "e|--3---|");
        let frame = app.frame(900.0, 600.0);
        assert!(frame.fits);
        assert_eq!(count(&frame, "INTRO"), 0);
        assert_eq!(count(&frame, "e|--3---|"), 0);
        assert_eq!(count(&frame, "Alpha"), 1);
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
        let frame = at_step(&app, 190.0, 8);
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
    fn tablature_stays_hidden_in_scroll_mode() {
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
        assert!(first.text.iter().all(|item| item.content != "TAB"));
        let (last, _) = app.frame_scrolled(450.0, 260.0, max_offset);
        assert!(last.text.iter().all(|item| item.content != "e|--3---|"));
        assert_eq!(count(&last, "Line 17"), 1);
    }

    #[test]
    fn first_scroll_view_shows_the_intro_after_a_hidden_tab() {
        let mut source = String::from(
            "{title: Test}\n{start_of_tab: Tab}\ne|--3---|\n{end_of_tab}\n{start_of_verse: Intro}\n[G] [Am] [C] [G]\n{end_of_verse}\n{start_of_verse: Verse 1}\n",
        );
        for index in 0..18 {
            source.push_str(&format!("[C]Line {index}\n"));
        }
        source.push_str("{end_of_verse}");
        let app = app(&source);
        let (first, max_offset) = app.frame_scrolled(450.0, 260.0, 0.0);
        assert!(max_offset > 0.0);
        assert_eq!(count(&first, "INTRO"), 1);
        assert_eq!(count(&first, "Am"), 1);
        assert_eq!(count(&first, "e|--3---|"), 0);
        assert!(!first.text.iter().any(|item| item.content.trim().is_empty()));
    }

    #[test]
    fn plain_text_after_a_hidden_tab_remains_visible() {
        let app = app(
            "{title: Test}\n{start_of_tab}\ne|--3--|\n{end_of_tab}\nSlide up on repeat\n{start_of_verse: Verse 1}\n[C]Hello\n{end_of_verse}",
        );
        let frame = app.frame(900.0, 600.0);
        assert_eq!(count(&frame, "e|--3--|"), 0);
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
        let frame = at_step(&app, 190.0, 8);
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
            let status = view.status.as_ref().expect("scroll status");
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
            Config::default(),
            Polarity::Light,
            &Fixed,
        );
        let (frame, max_offset) = wide_title.frame_scrolled(220.0, 300.0, 0.0);
        assert!(max_offset > 0.0);
        assert_eq!(count(&frame, "Line 0"), 1);
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
        assert!(view.status.is_none());
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
}
