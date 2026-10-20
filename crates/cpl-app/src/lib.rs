//! Headless song-page composition. No window or GPU types cross this boundary.

use cpl_song::{SectionKind, Song};

pub trait Measure {
    fn width(&self, text: &str, size_pt: f32, weight: u16) -> f32;
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
    pub ink: Ink,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub polarity: Polarity,
    pub text: Vec<Text>,
    pub fits: bool,
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
    pub chord_gap: f32,
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
            chord_gap: take!("chord_gap"),
            column_gutter: take!("column_gutter"),
        };
        if let Some(key) = values.keys().next() {
            return Err(format!("unknown page key: {key}"));
        }
        if config.chord_baseline >= config.lyric_baseline
            || config.lyric_baseline >= config.line_height
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

impl App {
    pub fn frame(&self, width: f32, height: f32, measure: &impl Measure) -> Frame {
        let c = &self.config;
        let body = c.body_size;
        let mut frame = Frame {
            width,
            height,
            polarity: self.polarity,
            text: Vec::new(),
            fits: true,
        };
        let title = self.song.first_meta("title").unwrap_or("Untitled");
        let title_size = c.title_size * body;
        let mut y = c.margin_top + title_size;
        frame.text.push(Text {
            content: title.into(),
            x: c.margin_x,
            baseline: y,
            size: title_size,
            weight: 700,
            ink: Ink::Strong,
        });
        if let Some(artist) = self.song.first_meta("artist") {
            y += c.subtitle_size * body * 1.8;
            frame.text.push(Text {
                content: artist.into(),
                x: c.margin_x,
                baseline: y,
                size: c.subtitle_size * body,
                weight: 400,
                ink: Ink::Soft,
            });
        }
        y += c.header_gap * body;
        let content_end = height - c.margin_bottom - body * 1.5;
        let right = width - c.margin_x;

        for (section_index, section) in self.song.sections.iter().enumerate() {
            if section_index > 0 {
                y += c.section_gap * body;
            }
            if !section.label.is_empty() {
                y += c.label_height * body;
                frame.text.push(Text {
                    content: section.label.to_uppercase(),
                    x: c.margin_x,
                    baseline: y,
                    size: c.label_size * body,
                    weight: if section.kind == SectionKind::Chorus {
                        700
                    } else {
                        600
                    },
                    ink: Ink::Soft,
                });
            }
            for line in &section.lines {
                let top = y;
                let chord_y = top + c.chord_baseline * body;
                let lyric_y = top + c.lyric_baseline * body;
                let mut lyric_x = c.margin_x;
                let mut chord_x = lyric_x;
                for segment in &line.segments {
                    if let Some(chord) = &segment.chord {
                        chord_x = chord_x.max(lyric_x);
                        frame.text.push(Text {
                            content: chord.clone(),
                            x: chord_x,
                            baseline: chord_y,
                            size: c.chord_size * body,
                            weight: 600,
                            ink: Ink::Soft,
                        });
                        chord_x +=
                            measure.width(chord, c.chord_size * body, 600) + c.chord_gap * body;
                    }
                    if !segment.lyric.is_empty() {
                        frame.text.push(Text {
                            content: segment.lyric.clone(),
                            x: lyric_x,
                            baseline: lyric_y,
                            size: body,
                            weight: 400,
                            ink: Ink::Strong,
                        });
                        lyric_x += measure.width(&segment.lyric, body, 400);
                    }
                    lyric_x = lyric_x.max(chord_x - c.chord_gap * body);
                }
                if lyric_x > right || lyric_y > content_end {
                    frame.fits = false;
                }
                y += c.line_height * body;
            }
        }
        if !frame.fits {
            frame
                .text
                .retain(|item| item.baseline < c.margin_top + title_size + c.header_gap * body);
            frame.text.push(Text {
                content: "Song needs paging at this readable size".into(),
                x: c.margin_x,
                baseline: y.min(content_end),
                size: body,
                weight: 600,
                ink: Ink::Soft,
            });
        }
        frame.text.push(Text {
            content: if frame.fits {
                "FIT  ·  stand 17 pt"
            } else {
                "PAGE NEEDED  ·  stand 17 pt"
            }
            .into(),
            x: c.margin_x,
            baseline: height - c.margin_bottom * 0.5,
            size: body * 0.7,
            weight: 400,
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
        fn width(&self, text: &str, size_pt: f32, _: u16) -> f32 {
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
    fn default_config_is_complete() {
        assert!(Config::parse(include_str!("../../../config/default.toml")).is_ok());
        assert!(Config::parse("[page]\nmargin_x = 2\nunknown = 3").is_err());
    }
}
