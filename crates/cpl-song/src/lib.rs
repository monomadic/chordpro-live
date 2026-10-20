//! Platform-independent song model and an initial ChordPro parser slice.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Song {
    pub metadata: BTreeMap<String, Vec<String>>,
    pub sections: Vec<Section>,
}

impl Song {
    pub fn first_meta(&self, name: &str) -> Option<&str> {
        self.metadata.get(name)?.first().map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub label: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Verse,
    Chorus,
    Bridge,
    Tab,
    Unlabelled,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub segments: Vec<Segment>,
}

impl Line {
    pub fn lyrics(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.lyric.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub chord: Option<String>,
    pub lyric: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResult {
    pub song: Song,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses common metadata, section pairs and bracketed chord markers.
/// Unsupported directives are diagnosed rather than silently discarded.
pub fn parse(source: &str) -> ParseResult {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut song = Song::default();
    let mut diagnostics = Vec::new();
    let mut active: Option<Section> = None;

    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        let text = raw.trim_end_matches('\r');
        let trimmed = text.trim();
        let in_tab = active
            .as_ref()
            .is_some_and(|section| section.kind == SectionKind::Tab);
        if !in_tab && (trimmed.is_empty() || trimmed.starts_with('#')) {
            continue;
        }
        if (!in_tab || trimmed == "{end_of_tab}")
            && let Some(body) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
        {
            let (name, value) = body
                .split_once(':')
                .map_or((body.trim(), ""), |(n, v)| (n.trim(), v.trim()));
            let name = match name {
                "t" => "title",
                "st" => "subtitle",
                "a" => "artist",
                "sov" => "start_of_verse",
                "eov" => "end_of_verse",
                "soc" => "start_of_chorus",
                "eoc" => "end_of_chorus",
                other => other,
            };
            match name {
                "start_of_verse" | "start_of_chorus" | "start_of_bridge" | "start_of_tab" => {
                    if let Some(section) = active.take() {
                        song.sections.push(section);
                        diagnostics.push(Diagnostic {
                            line: number,
                            message: "section started before previous section ended".into(),
                        });
                    }
                    let kind = match name {
                        "start_of_verse" => SectionKind::Verse,
                        "start_of_chorus" => SectionKind::Chorus,
                        "start_of_tab" => SectionKind::Tab,
                        _ => SectionKind::Bridge,
                    };
                    active = Some(Section {
                        kind,
                        label: if kind == SectionKind::Tab && value.is_empty() {
                            "Tab".into()
                        } else {
                            value.into()
                        },
                        lines: Vec::new(),
                    });
                }
                "end_of_verse" | "end_of_chorus" | "end_of_bridge" | "end_of_tab" => {
                    if let Some(section) = active.take() {
                        song.sections.push(section);
                    } else {
                        diagnostics.push(Diagnostic {
                            line: number,
                            message: "section end without a start".into(),
                        });
                    }
                }
                "title" | "subtitle" | "artist" | "composer" | "lyricist" | "arranger"
                | "copyright" | "album" | "year" | "key" | "time" | "tempo" | "duration"
                | "capo" | "sorttitle" | "tag" => {
                    song.metadata
                        .entry(name.into())
                        .or_default()
                        .push(value.into());
                }
                _ => diagnostics.push(Diagnostic {
                    line: number,
                    message: format!("unsupported directive: {name}"),
                }),
            }
            continue;
        }
        let line = if in_tab {
            Line {
                segments: vec![Segment {
                    chord: None,
                    lyric: text.into(),
                }],
            }
        } else {
            parse_line(text, number, &mut diagnostics)
        };
        active
            .get_or_insert_with(|| Section {
                kind: SectionKind::Unlabelled,
                label: String::new(),
                lines: Vec::new(),
            })
            .lines
            .push(line);
    }
    if let Some(section) = active {
        song.sections.push(section);
    }
    ParseResult { song, diagnostics }
}

fn parse_line(text: &str, number: usize, diagnostics: &mut Vec<Diagnostic>) -> Line {
    let mut segments = Vec::new();
    let mut remaining = text;
    while let Some(open) = remaining.find('[') {
        if open > 0 {
            segments.push(Segment {
                chord: None,
                lyric: remaining[..open].into(),
            });
        }
        let after = &remaining[open + 1..];
        let Some(close) = after.find(']') else {
            diagnostics.push(Diagnostic {
                line: number,
                message: "unclosed chord bracket".into(),
            });
            remaining = &remaining[open..];
            break;
        };
        let chord = &after[..close];
        if chord.is_empty() {
            diagnostics.push(Diagnostic {
                line: number,
                message: "empty chord bracket".into(),
            });
        }
        remaining = &after[close + 1..];
        let next = remaining.find('[').unwrap_or(remaining.len());
        segments.push(Segment {
            chord: Some(chord.into()),
            lyric: remaining[..next].into(),
        });
        remaining = &remaining[next..];
    }
    if !remaining.is_empty() {
        segments.push(Segment {
            chord: None,
            lyric: remaining.into(),
        });
    }
    Line { segments }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_attach_to_following_lyrics() {
        let result = parse(include_str!(
            "../../../testdata/house_of_the_rising_sun.cho"
        ));
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            result.song.first_meta("title"),
            Some("House of the Rising Sun")
        );
        let line = &result.song.sections[0].lines[0];
        assert_eq!(line.lyrics(), "There is a house in New Orleans");
        assert_eq!(line.segments[1].chord.as_deref(), Some("Am"));
        assert_eq!(line.segments[1].lyric, "is a ");
    }

    #[test]
    fn unsupported_is_visible() {
        let result = parse("{title: Test}\n{grid: C | G}\n");
        assert_eq!(result.diagnostics[0].line, 2);
        assert!(result.diagnostics[0].message.contains("grid"));
    }

    #[test]
    fn utf8_bom_and_crlf() {
        let result = parse("\u{feff}{t: Café}\r\n{soc: Chorus}\r\n[C]Crème\r\n{eoc}\r\n");
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.song.first_meta("title"), Some("Café"));
        assert_eq!(result.song.sections[0].lines[0].lyrics(), "Crème");
    }

    #[test]
    fn tab_keeps_brackets_and_spacing_verbatim() {
        let result = parse("{start_of_tab: Intro}\ne|--[3]---  |\n{end_of_tab}");
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.song.sections[0].kind, SectionKind::Tab);
        assert_eq!(result.song.sections[0].lines[0].lyrics(), "e|--[3]---  |");
        assert_eq!(result.song.sections[0].lines[0].segments[0].chord, None);
    }
}
