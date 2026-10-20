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
    pub fold_tabs: bool,
    pub hide_title: bool,
    pub collapse_repeats: bool,
    pub inline_chords: bool,
}

/// Header positions in em: x from the left margin, baselines from the top margin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderLayout {
    pub title_baseline: Option<f32>,
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

    pub fn line_height(&self, section: &MeasuredSection, m: &PageMetrics) -> f32 {
        if self.inline(section) {
            m.inline_line_height
        } else {
            m.line_height
        }
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

    pub fn block(&self, section: &MeasuredSection, m: &PageMetrics) -> Block {
        let visible = self.visible_lines(section);
        let inline = self.inline(section);
        let line_width = section
            .lines
            .iter()
            .take(visible)
            .map(|line| line.width(inline))
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
                + self.line_height(section, m) * visible as f32
                + self.label_row(section, m),
        }
    }

    pub fn header(&self, header: &Header, m: &PageMetrics) -> HeaderLayout {
        let mut y = 0.0;
        let mut title_baseline = None;
        if !self.hide_title {
            y += m.title_size;
            title_baseline = Some(y);
        }
        let mut artist = None;
        if header.artist.is_some() {
            if self.one_line_header && !self.hide_title {
                artist = Some((header.title_width + m.header_inline_gap, y));
            } else {
                y += if self.hide_title {
                    m.subtitle_size
                } else {
                    m.subtitle_size * m.byline_leading
                };
                artist = Some((0.0, y));
            }
        }
        HeaderLayout {
            title_baseline,
            artist,
            height: y + m.header_gap,
        }
    }
}

/// One rung: its number in PROPOSAL.md's ladder, whether it can change this
/// song's page at all, and the option it turns on (cumulatively).
pub struct Step {
    pub number: u8,
    pub applies: fn(&MeasuredSong, &PageMetrics, Viewport) -> bool,
    pub apply: fn(&mut Options),
}

/// Least destructive first. Steps 4 and 6 (diagrams, metadata pills) have
/// nothing to remove yet; step 10 is the caller scrolling or paging.
pub const LADDER: [Step; 7] = [
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
        number: 5,
        applies: |song, _, _| song.sections.iter().any(|s| s.kind == SectionKind::Tab),
        apply: |o| o.fold_tabs = true,
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
}

/// Lays out one candidate at the body size. Arithmetic only.
pub fn layout(
    song: &MeasuredSong,
    m: &PageMetrics,
    view: Viewport,
    options: Options,
    step: u8,
) -> Solution {
    let header = options.header(&song.header, m);
    let header_width = if options.one_line_header && !options.hide_title {
        song.header.title_width
            + m.header_inline_gap
            + song
                .header
                .artist
                .as_ref()
                .map_or(0.0, |_| song.header.artist_width)
    } else {
        (if options.hide_title {
            0.0
        } else {
            song.header.title_width
        })
        .max(song.header.artist_width)
    };
    let blocks: Vec<Block> = song.sections.iter().map(|s| options.block(s, m)).collect();
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
        fits: fit.meets_floor && header_width <= view.width,
        header,
        blocks,
        fit,
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
            options.block(&song.sections[0], &PAGE).height,
            PAGE.line_height
        );
        let above = Options {
            fold_tabs: true,
            ..Options::default()
        };
        assert_eq!(
            above.block(&song.sections[0], &PAGE).height,
            PAGE.label_height + PAGE.label_gap
        );
    }

    #[test]
    fn one_line_header_puts_the_byline_on_the_title_baseline() {
        let song = song(SONGS[0]);
        let full = Options::default().header(&song.header, &PAGE);
        let one = Options {
            one_line_header: true,
            ..Options::default()
        }
        .header(&song.header, &PAGE);
        assert_eq!(one.artist.unwrap().1, one.title_baseline.unwrap());
        assert!(one.height < full.height);
    }

    #[test]
    fn wide_title_must_drop_before_page_can_fit() {
        let song = song("{title: A title wider than the page}\n[C]Hi");
        let view = Viewport {
            width: 7.0,
            height: 30.0,
        };
        assert!(!layout(&song, &PAGE, view, Options::default(), 0).fits);
        let chosen = solve(&song, &PAGE, view);
        assert!(chosen.fits);
        assert_eq!(chosen.step, 7);
        assert!(chosen.options.hide_title);
    }
}
