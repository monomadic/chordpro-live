//! Headless song-page composition. No window or GPU types cross this boundary.

use cpl_song::{SectionKind, Song};

pub trait Measure {
    fn width(&self, text: &str, size_pt: f32, weight: u16, face: Face) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    System,
    Monospace,
}

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
        Ok(config)
    }
}

pub struct App {
    pub song: Song,
    pub config: Config,
    pub polarity: Polarity,
}

#[derive(Clone, Copy)]
struct Candidate {
    section_gap: f32,
    side_labels: bool,
    collapsed_header: bool,
    fold_tabs: bool,
    hide_title: bool,
    collapse_repeats: bool,
    inline_chords: bool,
    ladder_step: u8,
}

impl App {
    pub fn frame(&self, width: f32, height: f32, measure: &impl Measure) -> Frame {
        let full_options = Candidate {
            section_gap: self.config.section_gap,
            side_labels: false,
            collapsed_header: false,
            fold_tabs: false,
            hide_title: false,
            collapse_repeats: false,
            inline_chords: false,
            ladder_step: 0,
        };
        let full = self.frame_candidate(width, height, measure, full_options);
        if full.fits {
            return full;
        }
        let compact_options = Candidate {
            section_gap: 0.0,
            ladder_step: 1,
            ..full_options
        };
        let compact = self.frame_candidate(width, height, measure, compact_options);
        if compact.fits {
            return compact;
        }
        let side_allowed = self.labels_fit_side(measure);
        let side_options = Candidate {
            side_labels: side_allowed,
            ladder_step: 2,
            ..compact_options
        };
        if side_allowed {
            let side = self.frame_candidate(width, height, measure, side_options);
            if side.fits {
                return side;
            }
        }
        let header_options = Candidate {
            collapsed_header: true,
            ladder_step: 3,
            ..side_options
        };
        let header = self.frame_candidate(width, height, measure, header_options);
        if header.fits {
            return header;
        }
        let tabs_options = Candidate {
            fold_tabs: true,
            ladder_step: 5,
            ..header_options
        };
        if self
            .song
            .sections
            .iter()
            .any(|section| section.kind == SectionKind::Tab)
        {
            let tabs = self.frame_candidate(width, height, measure, tabs_options);
            if tabs.fits {
                return tabs;
            }
        }
        let no_title = Candidate {
            hide_title: true,
            ladder_step: 7,
            ..tabs_options
        };
        let title_dropped = self.frame_candidate(width, height, measure, no_title);
        if title_dropped.fits {
            return title_dropped;
        }
        let repeated = self.frame_candidate(
            width,
            height,
            measure,
            Candidate {
                collapse_repeats: true,
                ladder_step: 8,
                ..no_title
            },
        );
        if repeated.fits {
            return repeated;
        }
        self.frame_candidate(
            width,
            height,
            measure,
            Candidate {
                inline_chords: true,
                ladder_step: 9,
                collapse_repeats: true,
                ..no_title
            },
        )
    }

    fn is_repeated_chorus(&self, index: usize) -> bool {
        let section = &self.song.sections[index];
        section.kind == SectionKind::Chorus
            && self.song.sections[..index].iter().any(|earlier| {
                earlier.kind == SectionKind::Chorus && earlier.lines == section.lines
            })
    }

    fn labels_fit_side(&self, measure: &impl Measure) -> bool {
        let c = &self.config;
        let body = c.body_size;
        let room = c.margin_x.min(c.column_gutter * body) - body * 0.25;
        self.song.sections.iter().all(|section| {
            measure.width(
                &section.label.to_uppercase(),
                c.label_size * body,
                700,
                Face::Monospace,
            ) <= room
        })
    }

    fn frame_candidate(
        &self,
        width: f32,
        height: f32,
        measure: &impl Measure,
        candidate: Candidate,
    ) -> Frame {
        let Candidate {
            section_gap,
            side_labels,
            collapsed_header,
            fold_tabs,
            hide_title,
            collapse_repeats,
            inline_chords,
            ladder_step,
        } = candidate;
        let c = &self.config;
        let body = c.body_size;
        let mut frame = Frame {
            width,
            height,
            polarity: self.polarity,
            text: Vec::new(),
            fits: true,
            columns: 0,
            ladder_step,
        };
        let title = self.song.first_meta("title").unwrap_or("Untitled");
        let title_size = c.title_size * body;
        let mut y = c.margin_top;
        if !hide_title {
            y += title_size;
            frame.text.push(Text {
                content: title.into(),
                x: c.margin_x,
                baseline: y,
                size: title_size,
                weight: 600,
                face: Face::System,
                ink: Ink::Strong,
            });
        }
        if let Some(artist) = self.song.first_meta("artist") {
            let artist_x = if collapsed_header && !hide_title {
                c.margin_x + measure.width(title, title_size, 600, Face::System) + body * 0.8
            } else {
                y += if hide_title {
                    c.subtitle_size * body
                } else {
                    c.subtitle_size * body * 1.8
                };
                c.margin_x
            };
            frame.text.push(Text {
                content: artist.into(),
                x: artist_x,
                baseline: y,
                size: c.subtitle_size * body,
                weight: 400,
                face: Face::System,
                ink: Ink::Soft,
            });
        }
        y += c.header_gap * body;
        let content_end = height - c.margin_bottom - body * 1.5;
        let blocks: Vec<cpl_layout::Block> = self
            .song
            .sections
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let visible_lines = if fold_tabs && section.kind == SectionKind::Tab {
                    0
                } else if collapse_repeats && self.is_repeated_chorus(index) {
                    section.lines.len().min(1)
                } else {
                    section.lines.len()
                };
                let label_width = measure.width(
                    &section.label.to_uppercase(),
                    c.label_size * body,
                    700,
                    Face::Monospace,
                ) / body;
                let line_width = section
                    .lines
                    .iter()
                    .take(visible_lines)
                    .map(|line| {
                        line_width(
                            line,
                            c,
                            measure,
                            inline_chords,
                            section.kind == SectionKind::Tab,
                        ) / body
                    })
                    .fold(0.0, f32::max);
                cpl_layout::Block {
                    width: if side_labels {
                        line_width
                    } else {
                        label_width.max(line_width)
                    },
                    height: (if section.label.is_empty() || side_labels {
                        0.0
                    } else {
                        c.label_height + c.label_gap
                    }) + (if fold_tabs && section.kind == SectionKind::Tab && side_labels {
                        c.line_height
                    } else if inline_chords {
                        c.inline_line_height
                    } else {
                        c.line_height
                    }) * visible_lines as f32,
                }
            })
            .collect();
        let base_options = cpl_layout::FitOptions {
            available_width: ((width - 2.0 * c.margin_x) / body).max(0.0),
            available_height: ((content_end - y) / body).max(0.0),
            header_height: 0.0,
            header_gap: 0.0,
            section_gap,
            column_gutter: c.column_gutter,
            max_columns: 4,
            max_body_size: 1.0,
            floor: 1.0,
        };
        let fit = cpl_layout::fit(&blocks, base_options);
        frame.fits = fit.meets_floor;
        if side_labels {
            frame.fits &= self.labels_fit_side(measure);
        }
        if collapsed_header
            && !hide_title
            && let Some(artist) = self.song.first_meta("artist")
        {
            let title_width = measure.width(title, title_size, 600, Face::System);
            let artist_width = measure.width(artist, c.subtitle_size * body, 400, Face::System);
            frame.fits &= title_width + artist_width + body * 0.8 <= width - 2.0 * c.margin_x;
        }
        frame.columns = if frame.fits { fit.columns.len() } else { 0 };
        if frame.fits {
            let mut column_x = c.margin_x;
            for range in &fit.columns {
                let mut section_y = y;
                for (offset, index) in range.clone().enumerate() {
                    let section = &self.song.sections[index];
                    if offset > 0 {
                        section_y += section_gap * body;
                    }
                    if !section.label.is_empty() {
                        if !side_labels {
                            section_y += c.label_height * body;
                        }
                        let label_width = measure.width(
                            &section.label.to_uppercase(),
                            c.label_size * body,
                            700,
                            Face::Monospace,
                        );
                        frame.text.push(Text {
                            content: section.label.to_uppercase(),
                            x: if side_labels {
                                column_x - label_width - body * 0.25
                            } else {
                                column_x
                            },
                            baseline: if side_labels {
                                section_y
                                    + if inline_chords {
                                        c.inline_lyric_baseline
                                    } else {
                                        c.lyric_baseline
                                    } * body
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
                        if !side_labels {
                            section_y += c.label_gap * body;
                        }
                    }
                    let visible_lines = if fold_tabs && section.kind == SectionKind::Tab {
                        0
                    } else if collapse_repeats && self.is_repeated_chorus(index) {
                        section.lines.len().min(1)
                    } else {
                        section.lines.len()
                    };
                    for line in section.lines.iter().take(visible_lines) {
                        if section.kind == SectionKind::Tab {
                            frame.text.push(Text {
                                content: line.lyrics(),
                                x: column_x,
                                baseline: section_y + c.lyric_baseline * body,
                                size: body,
                                weight: 400,
                                face: Face::Monospace,
                                ink: Ink::Strong,
                            });
                            section_y += c.line_height * body;
                            continue;
                        }
                        if inline_chords {
                            let mut x = column_x;
                            for segment in &line.segments {
                                if let Some(chord) = &segment.chord {
                                    frame.text.push(Text {
                                        content: chord.clone(),
                                        x,
                                        baseline: section_y + c.inline_chord_baseline * body,
                                        size: c.inline_chord_size * body,
                                        weight: 600,
                                        face: Face::Monospace,
                                        ink: Ink::Soft,
                                    });
                                    x += measure.width(
                                        chord,
                                        c.inline_chord_size * body,
                                        600,
                                        Face::Monospace,
                                    ) + c.inline_chord_gap * body;
                                }
                                if !segment.lyric.is_empty() {
                                    frame.text.push(Text {
                                        content: segment.lyric.clone(),
                                        x,
                                        baseline: section_y + c.inline_lyric_baseline * body,
                                        size: body,
                                        weight: 400,
                                        face: Face::System,
                                        ink: Ink::Strong,
                                    });
                                    x += measure.width(&segment.lyric, body, 400, Face::System);
                                }
                            }
                            section_y += c.inline_line_height * body;
                            continue;
                        }
                        let chord_y = section_y + c.chord_baseline * body;
                        let lyric_y = section_y + c.lyric_baseline * body;
                        let mut lyric_x = column_x;
                        let mut chord_x = column_x;
                        for segment in &line.segments {
                            if let Some(chord) = &segment.chord {
                                chord_x = chord_x.max(lyric_x);
                                frame.text.push(Text {
                                    content: chord.clone(),
                                    x: chord_x,
                                    baseline: chord_y,
                                    size: c.chord_size * body,
                                    weight: 600,
                                    face: Face::Monospace,
                                    ink: Ink::Soft,
                                });
                                chord_x +=
                                    measure.width(chord, c.chord_size * body, 600, Face::Monospace)
                                        + c.chord_gap * body;
                            }
                            if !segment.lyric.is_empty() {
                                frame.text.push(Text {
                                    content: segment.lyric.clone(),
                                    x: lyric_x,
                                    baseline: lyric_y,
                                    size: body,
                                    weight: 400,
                                    face: Face::System,
                                    ink: Ink::Strong,
                                });
                                lyric_x += measure.width(&segment.lyric, body, 400, Face::System);
                            }
                            lyric_x = lyric_x.max(chord_x - c.chord_gap * body);
                        }
                        section_y += c.line_height * body;
                    }
                }
                let column_width = blocks[range.clone()]
                    .iter()
                    .map(|b| b.width)
                    .fold(0.0, f32::max);
                column_x += (column_width + c.column_gutter) * body;
            }
        }
        if !frame.fits {
            frame.text.push(Text {
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

fn line_width(
    line: &cpl_song::Line,
    c: &Config,
    measure: &impl Measure,
    inline: bool,
    tab: bool,
) -> f32 {
    if tab {
        return measure.width(&line.lyrics(), c.body_size, 400, Face::Monospace);
    }
    if inline {
        return line
            .segments
            .iter()
            .map(|segment| {
                measure.width(&segment.lyric, c.body_size, 400, Face::System)
                    + segment.chord.as_ref().map_or(0.0, |chord| {
                        measure.width(
                            chord,
                            c.inline_chord_size * c.body_size,
                            600,
                            Face::Monospace,
                        ) + c.inline_chord_gap * c.body_size
                    })
            })
            .sum();
    }
    let mut lyric_x: f32 = 0.0;
    let mut chord_x: f32 = 0.0;
    for segment in &line.segments {
        if let Some(chord) = &segment.chord {
            chord_x = chord_x.max(lyric_x)
                + measure.width(chord, c.chord_size * c.body_size, 600, Face::Monospace)
                + c.chord_gap * c.body_size;
        }
        lyric_x += measure.width(&segment.lyric, c.body_size, 400, Face::System);
        lyric_x = lyric_x.max(chord_x - c.chord_gap * c.body_size);
    }
    lyric_x.max(chord_x - c.chord_gap * c.body_size)
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

    #[test]
    fn song_chords_precede_their_lyrics() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_verse: Verse 1}\n[C]Hello [G]world\n{end_of_verse}",
        )
        .song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(900.0, 600.0, &Fixed);
        assert!(frame.fits);
        let chord = frame.text.iter().find(|t| t.content == "C").unwrap();
        let lyric = frame.text.iter().find(|t| t.content == "Hello ").unwrap();
        assert!(chord.baseline < lyric.baseline);
        assert_eq!(chord.x, lyric.x);
    }

    #[test]
    fn never_draws_overflowing_lyrics_under_the_floor() {
        let song = cpl_song::parse("{title: Test}\n[C]Hello").song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Dark,
        };
        let frame = app.frame(100.0, 90.0, &Fixed);
        assert!(!frame.fits);
        assert!(!frame.text.iter().any(|t| t.content == "Hello"));
    }

    #[test]
    fn sections_flow_into_columns_without_shrinking_lyrics() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_verse: One}\n[C]Alpha\n{end_of_verse}\n{start_of_chorus: Two}\n[G]Bravo\n{end_of_chorus}\n{start_of_verse: Three}\n[Am]Charlie\n{end_of_verse}",
        ).song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(900.0, 230.0, &Fixed);
        assert!(frame.fits);
        let lyrics: Vec<_> = frame
            .text
            .iter()
            .filter(|t| ["Alpha", "Bravo", "Charlie"].contains(&t.content.as_str()))
            .collect();
        assert_eq!(lyrics.len(), 3);
        assert!(lyrics.windows(2).all(|pair| pair[0].x < pair[1].x));
        assert!(lyrics.iter().all(|item| item.size == 17.0));
    }

    #[test]
    fn removes_section_gap_before_falling_back_to_paging() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_verse: Verse 1}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: Verse 2}\n[G]Bravo\n{end_of_verse}",
        ).song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 285.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.columns, 1);
        assert_eq!(frame.ladder_step, 1);
        assert!(
            frame
                .text
                .iter()
                .any(|t| t.content == "Bravo" && t.size == 17.0)
        );
    }

    #[test]
    fn moves_short_labels_to_side_after_spacing_compacts() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: B}\n[G]Bravo\n{end_of_verse}",
        ).song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 245.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 2);
        let label = frame.text.iter().find(|item| item.content == "A").unwrap();
        let lyric = frame
            .text
            .iter()
            .find(|item| item.content == "Alpha")
            .unwrap();
        assert!(label.x < lyric.x);
    }

    #[test]
    fn collapses_header_after_side_labels() {
        let song = cpl_song::parse(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}\n{start_of_verse: B}\n[G]Bravo\n{end_of_verse}",
        ).song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 210.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 3);
        let title = frame
            .text
            .iter()
            .find(|item| item.content == "Test")
            .unwrap();
        let artist = frame
            .text
            .iter()
            .find(|item| item.content == "Ada")
            .unwrap();
        assert_eq!(title.baseline, artist.baseline);
    }

    #[test]
    fn long_labels_skip_side_margin_but_allow_header_collapse() {
        let song = cpl_song::parse(
            "{title: Test}\n{artist: Ada}\n{start_of_verse: Verse One}\n[C]Alpha\n{end_of_verse}",
        )
        .song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 205.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 3);
        let label = frame
            .text
            .iter()
            .find(|item| item.content == "VERSE ONE")
            .unwrap();
        let lyric = frame
            .text
            .iter()
            .find(|item| item.content == "Alpha")
            .unwrap();
        assert_eq!(label.x, lyric.x);
    }

    #[test]
    fn title_drops_only_after_earlier_candidates_fail() {
        let song =
            cpl_song::parse("{title: Test}\n{start_of_verse: Verse One}\n[C]Alpha\n{end_of_verse}")
                .song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 180.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 7);
        assert!(!frame.text.iter().any(|item| item.content == "Test"));
        assert!(
            frame
                .text
                .iter()
                .any(|item| item.content == "Alpha" && item.size == 17.0)
        );
    }

    #[test]
    fn repeat_chorus_keeps_its_first_line_at_step_eight() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_chorus: A}\n[C]Echo\n[G]Again\n{end_of_chorus}\n{start_of_chorus: A}\n[C]Echo\n[G]Again\n{end_of_chorus}",
        ).song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(190.0, 235.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 8);
        assert_eq!(
            frame
                .text
                .iter()
                .filter(|item| item.content == "Echo")
                .count(),
            2
        );
        assert_eq!(
            frame
                .text
                .iter()
                .filter(|item| item.content == "Again")
                .count(),
            1
        );
    }

    #[test]
    fn inline_chords_are_last_fit_that_keeps_lyrics_readable() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n[G]Bravo\n[Am]Charlie\n{end_of_verse}",
        )
        .song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let frame = app.frame(300.0, 200.0, &Fixed);
        assert!(frame.fits);
        assert_eq!(frame.ladder_step, 9);
        let chord = frame.text.iter().find(|item| item.content == "C").unwrap();
        let lyric = frame
            .text
            .iter()
            .find(|item| item.content == "Alpha")
            .unwrap();
        assert!(chord.baseline < lyric.baseline);
        assert!(chord.x < lyric.x);
        assert_eq!(lyric.size, 17.0);
    }

    #[test]
    fn tab_is_monospaced_and_folds_at_step_five() {
        let song = cpl_song::parse(
            "{title: Test}\n{start_of_tab: Intro}\ne|--[3]---|\nB|---1----|\nG|---0----|\n{end_of_tab}\n{start_of_verse: A}\n[C]Alpha\n{end_of_verse}",
        )
        .song;
        let app = App {
            song,
            config: Config::default(),
            polarity: Polarity::Light,
        };
        let full = app.frame(320.0, 600.0, &Fixed);
        let tab = full
            .text
            .iter()
            .find(|item| item.content == "e|--[3]---|")
            .unwrap();
        assert_eq!(tab.face, Face::Monospace);
        let folded = (170..450)
            .map(|height| app.frame(320.0, height as f32, &Fixed))
            .find(|frame| frame.fits && frame.ladder_step == 5)
            .expect("a viewport where folding tab makes the page fit");
        assert!(folded.text.iter().any(|item| item.content == "INTRO"));
        assert!(!folded.text.iter().any(|item| item.content == "e|--[3]---|"));
        assert!(folded.text.iter().any(|item| item.content == "Alpha"));
    }

    #[test]
    fn default_config_is_complete() {
        assert!(Config::parse(include_str!("../../../config/default.toml")).is_ok());
        assert!(Config::parse("[page]\nmargin_x = 2\nunknown = 3").is_err());
    }
}
