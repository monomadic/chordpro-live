//! The reduction ladder: page options, their geometry in em, and the search
//! that applies them least destructive first until the page fits.

use crate::{Block, Fit, FitOptions, Header, MeasuredSection, MeasuredSong, fit};
use cpl_song::SectionKind;

/// Page geometry in em (multiples of the body size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMetrics {
    pub title_size: f32,
    pub subtitle_size: f32,
    /// Byline advance under a title, in subtitle sizes.
    pub byline_leading: f32,
    pub header_gap: f32,
    /// Space between title and byline on a one-line header.
    pub header_inline_gap: f32,
    pub section_gap: f32,
    pub label_height: f32,
    pub label_gap: f32,
    pub line_height: f32,
    pub inline_line_height: f32,
    pub column_gutter: f32,
    /// Width a side label may take left of its column.
    pub side_label_room: f32,
    pub max_columns: usize,
    /// Indent of a wrapped line's continuation rows.
    pub wrap_indent: f32,
    /// Baseline advance between rows of a wrapped title, in title sizes.
    pub title_leading: f32,
    /// Metadata pills: a row's height, a pill's height, its inner padding,
    /// the gap between pills and between a pill's label and value
    /// (chordpro-tui/internal/pdf/layout.go's metaLH, pillH, pillPadX,
    /// pillGap, kvGap), and the pill text size for vertical centring.
    pub meta_row: f32,
    pub pill_height: f32,
    pub pill_pad: f32,
    pub pill_gap: f32,
    pub pill_kv_gap: f32,
    pub meta_size: f32,
    /// Chord diagrams (chordpro-tui/internal/pdf/layout.go's dStrGap,
    /// dFretGap, dFretRows, dNameLH, dMarkLH, dFingerLH, dCellGap, dRowGap,
    /// dTopGap): string and fret spacing, fret rows shown, the rows for the
    /// name, the o/x marks and the finger numbers, and the gaps between
    /// diagrams, between their rows, and above them.
    pub diagram_string_gap: f32,
    pub diagram_fret_gap: f32,
    pub diagram_frets: f32,
    pub diagram_name_row: f32,
    pub diagram_mark_row: f32,
    pub diagram_finger_row: f32,
    pub diagram_cell_gap: f32,
    pub diagram_row_gap: f32,
    pub diagram_top_gap: f32,
}

impl PageMetrics {
    /// A pill's outer width in em.
    pub fn pill_width(&self, pill: &crate::MeasuredPill) -> f32 {
        2.0 * self.pill_pad + pill.label_width + self.pill_kv_gap + pill.value_width
    }

    /// A diagram cell's width: the grid or the name, whichever is wider,
    /// plus the starting-fret note beside a shape above the nut.
    pub fn diagram_width(&self, diagram: &crate::MeasuredDiagram) -> f32 {
        let grid = (diagram.shape.frets.len() - 1) as f32 * self.diagram_string_gap;
        grid.max(diagram.name_width) + diagram.fret_label.as_ref().map_or(0.0, |(_, w)| w + 0.15)
    }

    /// A diagram cell's height: name, marks, frets, finger numbers.
    pub fn diagram_height(&self) -> f32 {
        self.diagram_name_row
            + self.diagram_mark_row
            + self.diagram_frets * self.diagram_fret_gap
            + self.diagram_finger_row
    }
}

/// The content box between the margins and above the status strip, in em.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

/// One choice per ladder step. `Options::default()` is the full page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Options {
    pub tight_sections: bool,
    pub side_labels: bool,
    pub one_line_header: bool,
    /// Ladder step 4: the chord diagrams leave the header.
    pub hide_diagrams: bool,
    pub fold_tabs: bool,
    pub hide_title: bool,
    /// Ladder step 6: the metadata pills drop.
    pub hide_pills: bool,
    pub collapse_repeats: bool,
    pub inline_chords: bool,
}

/// Header positions in em: x from the left margin, baselines from the top margin.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderLayout {
    /// The first title row's baseline; `None` when the title is hidden.
    pub title_baseline: Option<f32>,
    /// The title as drawn: each row's text and baseline. One row unless the
    /// title is wider than the content box.
    pub title_rows: Vec<(String, f32)>,
    /// The widest header row, in em.
    pub width: f32,
    /// Each shown pill's left edge and vertical centre, index-aligned with
    /// `Header::pills`; empty when the pills are hidden.
    pub pills: Vec<(f32, f32)>,
    /// Each shown diagram cell's left edge and top, index-aligned with
    /// `Header::diagrams`; empty when they are hidden.
    pub diagrams: Vec<(f32, f32)>,
    pub artist: Option<(f32, f32)>,
    /// Where the first section starts.
    pub height: f32,
}

impl Options {
    pub fn section_gap(&self, m: &PageMetrics) -> f32 {
        if self.tight_sections {
            0.0
        } else {
            m.section_gap
        }
    }

    /// Tablature keeps its own monospaced rows in every candidate.
    pub fn inline(&self, section: &MeasuredSection) -> bool {
        self.inline_chords && section.kind != SectionKind::Tab
    }

    pub fn visible_lines(&self, section: &MeasuredSection) -> usize {
        if self.fold_tabs && section.kind == SectionKind::Tab {
            0
        } else if self.collapse_repeats && section.repeats_earlier {
            section.lines.len().min(1)
        } else {
            section.lines.len()
        }
    }

    /// Height of one row of this line.
    pub fn row_height(
        &self,
        section: &MeasuredSection,
        line: &crate::MeasuredLine,
        m: &PageMetrics,
    ) -> f32 {
        if line.chord_only || self.inline(section) {
            m.inline_line_height
        } else {
            m.line_height
        }
    }

    /// The line flowed into rows no wider than `wrap` em where its words allow.
    pub fn rows(
        &self,
        section: &MeasuredSection,
        line: &crate::MeasuredLine,
        m: &PageMetrics,
        wrap: f32,
    ) -> Vec<crate::Row> {
        line.rows(self.inline(section), wrap, m.wrap_indent)
    }

    /// Height of the whole line, every wrapped row included.
    pub fn line_height(
        &self,
        section: &MeasuredSection,
        line: &crate::MeasuredLine,
        m: &PageMetrics,
        wrap: f32,
    ) -> f32 {
        self.row_height(section, line, m) * self.rows(section, line, m, wrap).len() as f32
    }

    /// A side label beside a section with every line hidden keeps one row,
    /// so it never sits over the next section.
    pub fn label_row(&self, section: &MeasuredSection, m: &PageMetrics) -> f32 {
        if self.side_labels && !section.label.is_empty() && self.visible_lines(section) == 0 {
            m.line_height
        } else {
            0.0
        }
    }

    /// The section's block with lines wrapped at `wrap` em.
    pub fn block(&self, section: &MeasuredSection, m: &PageMetrics, wrap: f32) -> Block {
        let visible = self.visible_lines(section);
        let line_width = section
            .lines
            .iter()
            .take(visible)
            .flat_map(|line| self.rows(section, line, m, wrap))
            .map(|row| row.width)
            .fold(0.0, f32::max);
        let label = if section.label.is_empty() || self.side_labels {
            0.0
        } else {
            m.label_height + m.label_gap
        };
        Block {
            width: if self.side_labels {
                line_width
            } else {
                section.label_width.max(line_width)
            },
            height: label
                + section
                    .lines
                    .iter()
                    .take(visible)
                    .map(|line| self.line_height(section, line, m, wrap))
                    .sum::<f32>()
                + self.label_row(section, m),
        }
    }

    /// The header for a content box `max_width` em wide. A title wider than
    /// that wraps at its word boundaries; only a single word wider than the
    /// box still overflows, and the width says so.
    pub fn header(&self, header: &Header, m: &PageMetrics, max_width: f32) -> HeaderLayout {
        let mut y = 0.0;
        let mut title_baseline = None;
        let mut title_rows = Vec::new();
        let mut width: f32 = 0.0;
        if !self.hide_title {
            let rows = title_rows_within(header, max_width);
            for (index, (text, row_width)) in rows.into_iter().enumerate() {
                y += if index == 0 {
                    m.title_size
                } else {
                    m.title_size * m.title_leading
                };
                title_baseline.get_or_insert(y);
                width = width.max(row_width);
                title_rows.push((text, y));
            }
        }
        let mut artist = None;
        if header.artist.is_some() {
            if self.one_line_header && !self.hide_title && title_rows.len() == 1 {
                artist = Some((header.title_width + m.header_inline_gap, y));
                width = header.title_width + m.header_inline_gap + header.artist_width;
            } else {
                y += if self.hide_title {
                    m.subtitle_size
                } else {
                    m.subtitle_size * m.byline_leading
                };
                artist = Some((0.0, y));
                width = width.max(header.artist_width);
            }
        }
        let mut pills = Vec::new();
        if !self.hide_pills && !header.pills.is_empty() {
            let widths: Vec<f32> = header.pills.iter().map(|p| m.pill_width(p)).collect();
            let total = widths.iter().sum::<f32>() + m.pill_gap * (widths.len() - 1) as f32;
            // Beside the title block, right-aligned on its last line, when the
            // block leaves room (the mock's placement); otherwise in rows of
            // their own below it.
            let has_block = y > 0.0;
            let last_size = if header.artist.is_some() {
                m.subtitle_size
            } else {
                m.title_size
            };
            if has_block && width + 2.0 * m.pill_gap + total <= max_width {
                let mut x = if max_width.is_finite() {
                    max_width - total
                } else {
                    width + 2.0 * m.pill_gap
                };
                let centre = y - last_size * 0.35;
                for pill_width in &widths {
                    pills.push((x, centre));
                    x += pill_width + m.pill_gap;
                }
                width = width.max(x - m.pill_gap);
            } else {
                let top = if has_block { y + 0.35 } else { y };
                let (mut x, mut row) = (0.0_f32, 0usize);
                for pill_width in &widths {
                    if x > 0.0 && x + pill_width > max_width + 1e-4 {
                        x = 0.0;
                        row += 1;
                    }
                    pills.push((x, top + m.meta_row * (row as f32 + 0.5)));
                    x += pill_width + m.pill_gap;
                    width = width.max(x - m.pill_gap);
                }
                y = top + m.meta_row * (row + 1) as f32;
            }
        }
        let mut diagrams = Vec::new();
        if !self.hide_diagrams && !header.diagrams.is_empty() {
            let top = if y > 0.0 { y + m.diagram_top_gap } else { y };
            let (mut x, mut row) = (0.0_f32, 0usize);
            for diagram in &header.diagrams {
                let cell = m.diagram_width(diagram);
                if x > 0.0 && x + cell > max_width + 1e-4 {
                    x = 0.0;
                    row += 1;
                }
                diagrams.push((
                    x,
                    top + row as f32 * (m.diagram_height() + m.diagram_row_gap),
                ));
                x += cell + m.diagram_cell_gap;
                width = width.max(x - m.diagram_cell_gap);
            }
            y = top + (row + 1) as f32 * m.diagram_height() + row as f32 * m.diagram_row_gap;
        }
        HeaderLayout {
            title_baseline,
            title_rows,
            width,
            pills,
            diagrams,
            artist,
            height: y + m.header_gap,
        }
    }
}

/// The title flowed into rows no wider than `max_width` em, breaking only
/// after a word that ends in whitespace. Each row's text drops its trailing
/// space; its width keeps it, which errs toward breaking a little early.
fn title_rows_within(header: &Header, max_width: f32) -> Vec<(String, f32)> {
    if header.title_width <= max_width || header.title_tokens.len() < 2 {
        return vec![(header.title.clone(), header.title_width)];
    }
    let mut rows: Vec<(String, f32)> = Vec::new();
    let (mut text, mut width) = (String::new(), 0.0_f32);
    for token in &header.title_tokens {
        if !text.is_empty() && width + token.width > max_width + 1e-4 {
            rows.push((text.trim_end().to_owned(), width));
            text.clear();
            width = 0.0;
        }
        text.push_str(&token.text);
        width += token.width;
    }
    rows.push((text.trim_end().to_owned(), width));
    rows
}

/// One rung: its number in PROPOSAL.md's ladder, whether it can change this
/// song's page at all, and the option it turns on (cumulatively).
pub struct Step {
    pub number: u8,
    pub applies: fn(&MeasuredSong, &PageMetrics, Viewport) -> bool,
    pub apply: fn(&mut Options),
}

/// Least destructive first; step 10 is the caller scrolling or paging.
pub const LADDER: [Step; 9] = [
    Step {
        number: 1,
        applies: |song, _, _| song.sections.len() > 1,
        apply: |o| o.tight_sections = true,
    },
    Step {
        number: 2,
        applies: labels_fit_side,
        apply: |o| o.side_labels = true,
    },
    Step {
        number: 3,
        applies: header_fits_one_line,
        apply: |o| o.one_line_header = true,
    },
    Step {
        number: 4,
        applies: |song, _, _| !song.header.diagrams.is_empty(),
        apply: |o| o.hide_diagrams = true,
    },
    Step {
        number: 5,
        applies: |song, _, _| song.sections.iter().any(|s| s.kind == SectionKind::Tab),
        apply: |o| o.fold_tabs = true,
    },
    Step {
        number: 6,
        applies: |song, _, _| !song.header.pills.is_empty(),
        apply: |o| o.hide_pills = true,
    },
    Step {
        number: 7,
        applies: |_, _, _| true,
        apply: |o| o.hide_title = true,
    },
    Step {
        number: 8,
        applies: |song, _, _| song.sections.iter().any(|s| s.repeats_earlier),
        apply: |o| o.collapse_repeats = true,
    },
    Step {
        number: 9,
        applies: |_, _, _| true,
        apply: |o| o.inline_chords = true,
    },
];

fn labels_fit_side(song: &MeasuredSong, m: &PageMetrics, _: Viewport) -> bool {
    song.sections.iter().any(|s| !s.label.is_empty())
        && song
            .sections
            .iter()
            .all(|s| s.label_width <= m.side_label_room)
}

fn header_fits_one_line(song: &MeasuredSong, m: &PageMetrics, view: Viewport) -> bool {
    let h = &song.header;
    h.artist.is_some() && h.title_width + m.header_inline_gap + h.artist_width <= view.width
}

/// A laid-out candidate: the options, the ladder step they represent, the
/// header, every section's block and the chosen columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub options: Options,
    pub step: u8,
    pub fits: bool,
    pub header: HeaderLayout,
    pub blocks: Vec<Block>,
    pub fit: Fit,
    /// Continuation rows across every visible line: how much wrapping this
    /// candidate needs at this width.
    pub wrapped_rows: usize,
}

/// Lays out one candidate at the body size. Arithmetic only.
pub fn layout(
    song: &MeasuredSong,
    m: &PageMetrics,
    view: Viewport,
    options: Options,
    step: u8,
) -> Solution {
    let header = options.header(&song.header, m, view.width);
    let header_width = header.width;
    // Lines wider than the content box wrap at its width; a block that is
    // still wider holds an unbreakable run, and the fit reports it.
    let blocks: Vec<Block> = song
        .sections
        .iter()
        .map(|s| options.block(s, m, view.width))
        .collect();
    let wrapped_rows = song
        .sections
        .iter()
        .flat_map(|section| {
            section
                .lines
                .iter()
                .take(options.visible_lines(section))
                .map(move |line| options.rows(section, line, m, view.width).len() - 1)
        })
        .sum();
    let fit = fit(
        &blocks,
        FitOptions {
            available_width: view.width.max(0.0),
            available_height: (view.height - header.height).max(0.0),
            header_height: 0.0,
            header_gap: 0.0,
            section_gap: options.section_gap(m),
            column_gutter: m.column_gutter,
            max_columns: m.max_columns,
            max_body_size: 1.0,
            floor: 1.0,
        },
    );
    Solution {
        options,
        step,
        // Side labels live in the margin, which is fixed in points, so at a
        // larger body size they may no longer fit it; such a candidate is
        // unfit rather than drawn off the page.
        fits: fit.meets_floor
            && header_width <= view.width
            && (!options.side_labels || labels_fit_side(song, m, view)),
        header,
        blocks,
        fit,
        wrapped_rows,
    }
}

/// The first candidate that fits, trying the full page and then every
/// applicable step in order. When nothing fits it returns the last candidate
/// tried, unfit, and the caller scrolls.
pub fn solve(song: &MeasuredSong, m: &PageMetrics, view: Viewport) -> Solution {
    let mut options = Options::default();
    let mut solution = layout(song, m, view, options, 0);
    for step in LADDER.iter().filter(|s| (s.applies)(song, m, view)) {
        if solution.fits {
            break;
        }
        (step.apply)(&mut options);
        solution = layout(song, m, view, options, step.number);
    }
    solution
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Face, Measure, Metrics, measure_song};

    #[test]
    fn a_candidate_counts_the_rows_wrapping_adds() {
        let song = song("[C]one two three four five six");
        let wide = layout(
            &song,
            &PAGE,
            Viewport {
                width: 100.0,
                height: 100.0,
            },
            Options::default(),
            0,
        );
        assert_eq!(wide.wrapped_rows, 0);
        let narrow = layout(
            &song,
            &PAGE,
            Viewport {
                width: 4.0,
                height: 100.0,
            },
            Options::default(),
            0,
        );
        assert!(narrow.wrapped_rows >= 2, "{}", narrow.wrapped_rows);
        assert!(narrow.fit.meets_floor, "wrapped lines fit the width");
        assert!(narrow.blocks[0].height > wide.blocks[0].height);
    }

    struct Fixed;
    impl Measure for Fixed {
        fn width(&self, text: &str, size_pt: f32, _: u16, _: Face) -> f32 {
            text.chars().count() as f32 * size_pt * 0.5
        }
    }

    const PAGE: PageMetrics = PageMetrics {
        title_size: 2.0,
        subtitle_size: 0.75,
        byline_leading: 1.8,
        header_gap: 1.0,
        header_inline_gap: 0.8,
        section_gap: 1.0,
        label_height: 1.5,
        label_gap: 0.5,
        line_height: 2.5,
        inline_line_height: 1.5,
        column_gutter: 3.0,
        side_label_room: 2.0,
        max_columns: 4,
        wrap_indent: 1.0,
        title_leading: 1.15,
        meta_row: 1.9,
        pill_height: 1.25,
        pill_pad: 0.42,
        pill_gap: 0.55,
        pill_kv_gap: 0.3,
        meta_size: 0.7,
        diagram_string_gap: 0.54,
        diagram_fret_gap: 0.6,
        diagram_frets: 4.0,
        diagram_name_row: 1.1,
        diagram_mark_row: 0.48,
        diagram_finger_row: 0.65,
        diagram_cell_gap: 1.1,
        diagram_row_gap: 0.55,
        diagram_top_gap: 0.65,
    };

    fn song(source: &str) -> MeasuredSong {
        let metrics = Metrics {
            reference_size: 10.0,
            title_size: PAGE.title_size,
            subtitle_size: PAGE.subtitle_size,
            label_size: 0.7,
            chord_size: 0.8,
            chord_gap: 0.4,
            inline_chord_size: 0.6,
            inline_chord_gap: 0.2,
            meta_size: 0.7,
            diagram_name_size: 0.78,
            diagram_mark_size: 0.42,
        };
        measure_song(&cpl_song::parse(source).song, &metrics, &Fixed)
    }

    const SONGS: [&str; 4] = [
        "{title: Test}\n{artist: Ada}\n{sov: A}\n[C]Alpha [G]one\n[Am]Bravo\n{eov}\n{soc: B}\n[F]Charlie\n[G]Delta\n{eoc}\n{sov: C}\n[C]Echo\n{eov}\n{soc: B}\n[F]Charlie\n[G]Delta\n{eoc}",
        "{title: A rather long song title}\n{artist: Somebody Famous}\n{start_of_tab: Intro}\ne|--3---|\nB|--1---|\nG|--0---|\n{end_of_tab}\n{sov: Verse One}\n[C]Alpha\n[G]Bravo\n{eov}",
        "{title: T}\n{start_of_tab: T}\ne|--3--|\nB|--1--|\n{end_of_tab}\n{sov: A}\n[C]Alpha\n[G]Bravo\n{eov}",
        "[C]Just one section of [G]lyrics\n[Am]and nothing else",
    ];

    /// Every candidate `solve` may try, in order, with its step number.
    fn candidates(song: &MeasuredSong, view: Viewport) -> Vec<(u8, Options)> {
        let mut options = Options::default();
        let mut all = vec![(0, options)];
        for step in LADDER.iter().filter(|s| (s.applies)(song, &PAGE, view)) {
            (step.apply)(&mut options);
            all.push((step.number, options));
        }
        all
    }

    fn views() -> impl Iterator<Item = Viewport> {
        (4..40).flat_map(|w| {
            (2..60).map(move |h| Viewport {
                width: w as f32 * 1.5,
                height: h as f32,
            })
        })
    }

    #[test]
    fn a_step_is_chosen_only_after_every_earlier_applicable_step_fails() {
        for source in SONGS {
            let song = song(source);
            for view in views() {
                let chosen = solve(&song, &PAGE, view);
                let tried = candidates(&song, view);
                let at = tried.iter().position(|&(n, _)| n == chosen.step).unwrap();
                for &(number, options) in &tried[..at] {
                    assert!(!layout(&song, &PAGE, view, options, number).fits);
                }
                if !chosen.fits {
                    assert_eq!(at, tried.len() - 1, "an unfit page has tried every step");
                }
            }
        }
    }

    #[test]
    fn a_shorter_viewport_never_needs_fewer_steps() {
        for source in SONGS {
            let song = song(source);
            for w in 4..40 {
                let mut previous = 0;
                for h in (2..60).rev() {
                    let view = Viewport {
                        width: w as f32 * 1.5,
                        height: h as f32,
                    };
                    let s = solve(&song, &PAGE, view);
                    let step = if s.fits { s.step } else { 10 };
                    assert!(step >= previous, "{source:?} at {view:?}");
                    previous = step;
                }
            }
        }
    }

    #[test]
    fn inapplicable_steps_are_never_chosen() {
        let song = song(SONGS[3]);
        for view in views() {
            let s = solve(&song, &PAGE, view);
            assert!(
                ![1, 2, 3, 5, 8].contains(&s.step),
                "{view:?} chose {}",
                s.step
            );
        }
    }

    #[test]
    fn a_header_too_wide_for_one_line_does_not_block_later_steps() {
        let song = song(SONGS[1]);
        let narrow = Viewport {
            width: song.header.title_width + 0.1,
            height: 0.0,
        };
        assert!(!header_fits_one_line(&song, &PAGE, narrow));
        let folded = (2..60).find_map(|h| {
            let s = solve(
                &song,
                &PAGE,
                Viewport {
                    height: h as f32,
                    ..narrow
                },
            );
            (s.fits && s.step == 5).then_some(s)
        });
        let folded = folded.expect("folding the tab fits some height");
        assert!(!folded.options.one_line_header);
        assert!(folded.header.title_baseline.is_some(), "the title is kept");
    }

    #[test]
    fn a_folded_tab_keeps_a_row_for_its_side_label() {
        let song = song(SONGS[2]);
        let options = Options {
            side_labels: true,
            fold_tabs: true,
            ..Options::default()
        };
        assert_eq!(
            options
                .block(&song.sections[0], &PAGE, f32::INFINITY)
                .height,
            PAGE.line_height
        );
        let above = Options {
            fold_tabs: true,
            ..Options::default()
        };
        assert_eq!(
            above.block(&song.sections[0], &PAGE, f32::INFINITY).height,
            PAGE.label_height + PAGE.label_gap
        );
    }

    #[test]
    fn one_line_header_puts_the_byline_on_the_title_baseline() {
        let song = song(SONGS[0]);
        let full = Options::default().header(&song.header, &PAGE, f32::INFINITY);
        let one = Options {
            one_line_header: true,
            ..Options::default()
        }
        .header(&song.header, &PAGE, f32::INFINITY);
        assert_eq!(one.artist.unwrap().1, one.title_baseline.unwrap());
        assert!(one.height < full.height);
    }

    #[test]
    fn a_wide_title_wraps_and_drops_only_for_height() {
        // Fixed: "A title wider than the page" is 27 em at title size 2.
        let song = song("{title: A title wider than the page}\nHi");
        let roomy = Viewport {
            width: 7.0,
            height: 30.0,
        };
        let chosen = solve(&song, &PAGE, roomy);
        assert!(chosen.fits);
        assert_eq!(chosen.step, 0, "a wrapped title needs no reduction");
        let rows = &chosen.header.title_rows;
        assert!(rows.len() > 1, "{rows:?}");
        assert!(chosen.header.width <= 7.0);
        assert!(
            rows.windows(2).all(|pair| pair[1].1 > pair[0].1),
            "rows descend"
        );
        let joined: Vec<&str> = rows.iter().map(|(text, _)| text.as_str()).collect();
        assert_eq!(joined.join(" "), "A title wider than the page");
        // Too short for the wrapped title: it drops at step 7, as before.
        let short = Viewport {
            width: 7.0,
            height: chosen.header.height + 1.0,
        };
        let chosen = solve(&song, &PAGE, short);
        assert!(chosen.fits);
        assert_eq!(chosen.step, 7);
        assert!(chosen.options.hide_title);
    }

    #[test]
    fn side_labels_too_wide_for_the_margin_make_a_candidate_unfit() {
        // Fixed: "ABCD" at label size 0.7 is 1.4 em; PAGE gives 2 em of room.
        let song = song("{sov: ABCD}\n[C]Alpha\n{eov}\n{sov: ABCD}\n[G]Bravo\n{eov}");
        let view = Viewport {
            width: 30.0,
            height: 30.0,
        };
        let side = Options {
            side_labels: true,
            ..Options::default()
        };
        assert!(layout(&song, &PAGE, view, side, 2).fits);
        let cramped = PageMetrics {
            side_label_room: 1.0,
            ..PAGE
        };
        assert!(!layout(&song, &cramped, view, side, 2).fits);
    }

    #[test]
    fn pills_sit_beside_the_title_when_there_is_room_and_wrap_below_otherwise() {
        let song = song("{title: Hi}\n{artist: Ada}\n{key: Am}\n{capo: 2}\n{tempo: 96}\n[C]Alpha");
        assert_eq!(song.header.pills.len(), 3);
        let wide = Options::default().header(&song.header, &PAGE, 40.0);
        let narrow = Options::default().header(&song.header, &PAGE, 6.0);
        assert_eq!(wide.pills.len(), 3);
        // Beside: one row, right-aligned, no extra height.
        assert!(wide.pills.iter().all(|&(_, c)| c == wide.pills[0].1));
        let last = song.header.pills.last().unwrap();
        let right = wide.pills.last().unwrap().0 + PAGE.pill_width(last);
        assert!(
            (right - 40.0).abs() < 1e-4,
            "right-aligned to the content edge"
        );
        // Below: under the byline, wrapping, and the header grows.
        assert!(narrow.pills[0].1 > narrow.artist.unwrap().1);
        assert!(narrow.height > wide.height);
        assert!(narrow.width <= 6.0 + 1e-4);
        let hidden = Options {
            hide_pills: true,
            ..Options::default()
        }
        .header(&song.header, &PAGE, 6.0);
        assert!(hidden.pills.is_empty());
        assert!(hidden.height < narrow.height);
    }

    #[test]
    fn pills_drop_at_step_six_after_tabs_fold_and_before_the_title() {
        let measured = song(
            "{title: Hi}\n{key: Am}\n{capo: 2}\n{tempo: 96}\n{time: 3/4}\n{sov: A}\n[C]Alpha\n[G]Beta\n{eov}",
        );
        let tried: Vec<u8> = LADDER
            .iter()
            .filter(|s| {
                (s.applies)(
                    &measured,
                    &PAGE,
                    Viewport {
                        width: 8.0,
                        height: 9.0,
                    },
                )
            })
            .map(|s| s.number)
            .collect();
        let six = tried.iter().position(|&n| n == 6).expect("step 6 applies");
        let seven = tried.iter().position(|&n| n == 7).unwrap();
        assert!(six < seven);
        let without = song("{title: Hi}\n[C]Alpha");
        assert!(!LADDER.iter().any(|s| s.number == 6
            && (s.applies)(
                &without,
                &PAGE,
                Viewport {
                    width: 8.0,
                    height: 9.0
                }
            )));
        // Some viewport needs exactly step 6 to fit.
        let found = (8..40).find_map(|h| {
            let s = solve(
                &measured,
                &PAGE,
                Viewport {
                    width: 8.0,
                    height: h as f32,
                },
            );
            (s.fits && s.step == 6).then_some(s)
        });
        let chosen = found.expect("a height that needs the pills gone");
        assert!(chosen.options.hide_pills && !chosen.options.hide_title);
        assert!(chosen.header.pills.is_empty());
    }

    #[test]
    fn diagrams_flow_in_rows_and_drop_at_step_four() {
        let measured = song("{title: Hi}\n{sov: A}\n[C]One [G]two [Am]three [F]four\n{eov}");
        assert_eq!(
            measured
                .header
                .diagrams
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["C", "G", "Am", "F"]
        );
        let wide = Options::default().header(&measured.header, &PAGE, 40.0);
        let narrow = Options::default().header(&measured.header, &PAGE, 6.0);
        assert_eq!(wide.diagrams.len(), 4);
        assert!(
            wide.diagrams
                .iter()
                .all(|&(_, top)| top == wide.diagrams[0].1),
            "one row"
        );
        assert!(
            narrow
                .diagrams
                .iter()
                .any(|&(_, top)| top > narrow.diagrams[0].1),
            "wraps"
        );
        assert!(narrow.height > wide.height);
        let hidden = Options {
            hide_diagrams: true,
            ..Options::default()
        }
        .header(&measured.header, &PAGE, 40.0);
        assert!(hidden.diagrams.is_empty() && hidden.height < wide.height);
        let view = Viewport {
            width: 30.0,
            height: 9.0,
        };
        let order: Vec<u8> = LADDER
            .iter()
            .filter(|s| (s.applies)(&measured, &PAGE, view))
            .map(|s| s.number)
            .collect();
        let four = order.iter().position(|&n| n == 4).expect("step 4 applies");
        assert!(order[..four].iter().all(|&n| n < 4) && order[four + 1..].iter().all(|&n| n > 4));
        let found = (5..40).find_map(|h| {
            let s = solve(
                &measured,
                &PAGE,
                Viewport {
                    width: 30.0,
                    height: h as f32,
                },
            );
            (s.fits && s.step == 4).then_some(s)
        });
        let chosen = found.expect("a height that needs the diagrams gone");
        assert!(chosen.options.hide_diagrams && chosen.header.diagrams.is_empty());
    }

    #[test]
    fn a_single_word_title_wider_than_the_page_still_does_not_fit() {
        let song = song("{title: Supercalifragilistic}\n[C]Hi");
        let view = Viewport {
            width: 7.0,
            height: 30.0,
        };
        assert!(!layout(&song, &PAGE, view, Options::default(), 0).fits);
        assert!(solve(&song, &PAGE, view).options.hide_title);
    }
}
