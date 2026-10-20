//! Measures a song once into em-unit runs. This is the only layout stage that
//! calls the OS; fitting and placement are arithmetic over these numbers.

use cpl_song::{Line, SectionKind, Song};
use std::collections::HashMap;

pub trait Measure {
    /// Advance width in points of `text` set at `size_pt`.
    fn width(&self, text: &str, size_pt: f32, weight: u16, face: Face) -> f32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    System,
    Monospace,
}

/// What a run of text is. The role alone fixes its face, weight and em size,
/// so measurement and drawing cannot disagree about how a run is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Title,
    Subtitle,
    Label,
    Chord,
    Lyric,
    InlineChord,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Multiple of the body size.
    pub size: f32,
    pub weight: u16,
    pub face: Face,
}

/// The type scale measurement needs. Sizes and gaps are in em; the reference
/// size is the point size measured at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Text is measured at this body size. System-font tracking varies with
    /// point size, so em widths are exact here and close nearby.
    pub reference_size: f32,
    pub title_size: f32,
    pub subtitle_size: f32,
    pub label_size: f32,
    pub chord_size: f32,
    pub chord_gap: f32,
    pub inline_chord_size: f32,
    pub inline_chord_gap: f32,
}

impl Metrics {
    pub fn style(&self, role: Role) -> Style {
        let (size, weight, face) = match role {
            Role::Title => (self.title_size, 600, Face::System),
            Role::Subtitle => (self.subtitle_size, 400, Face::System),
            Role::Label => (self.label_size, 700, Face::Monospace),
            Role::Chord => (self.chord_size, 600, Face::Monospace),
            Role::Lyric => (1.0, 400, Face::System),
            Role::InlineChord => (self.inline_chord_size, 600, Face::Monospace),
            Role::Tab => (1.0, 400, Face::Monospace),
        };
        Style { size, weight, face }
    }
}

/// A positioned run, `x` and `width` in em from the start of its line.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub role: Role,
    pub text: String,
    pub x: f32,
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredLine {
    /// Chords on their own row above the lyric, overhang pushing lyrics right.
    pub above: Vec<Run>,
    pub above_width: f32,
    /// Small chords set inline before their lyric segment (ladder step 9).
    pub inline: Vec<Run>,
    pub inline_width: f32,
}

impl MeasuredLine {
    pub fn runs(&self, inline: bool) -> &[Run] {
        if inline { &self.inline } else { &self.above }
    }

    pub fn width(&self, inline: bool) -> f32 {
        if inline {
            self.inline_width
        } else {
            self.above_width
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredSection {
    pub kind: SectionKind,
    /// Upper-cased, as drawn.
    pub label: String,
    pub label_width: f32,
    /// A chorus whose lines equal an earlier chorus (ladder step 8).
    pub repeats_earlier: bool,
    pub lines: Vec<MeasuredLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub title: String,
    pub title_width: f32,
    pub artist: Option<String>,
    pub artist_width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredSong {
    pub header: Header,
    pub sections: Vec<MeasuredSection>,
}

/// Measures every run once per song and font set. Equal strings in the same
/// role are measured once, which matters because chord names repeat.
pub fn measure_song(song: &Song, metrics: &Metrics, measure: &impl Measure) -> MeasuredSong {
    let mut m = Measurer {
        measure,
        metrics,
        cache: HashMap::new(),
    };
    let title = song.first_meta("title").unwrap_or("Untitled").to_owned();
    let artist = song.first_meta("artist").map(str::to_owned);
    let header = Header {
        title_width: m.em(&title, Role::Title),
        artist_width: artist.as_deref().map_or(0.0, |a| m.em(a, Role::Subtitle)),
        title,
        artist,
    };
    let sections = song
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            let label = section.label.to_uppercase();
            MeasuredSection {
                kind: section.kind,
                label_width: m.em(&label, Role::Label),
                label,
                repeats_earlier: section.kind == SectionKind::Chorus
                    && song.sections[..index].iter().any(|earlier| {
                        earlier.kind == SectionKind::Chorus && earlier.lines == section.lines
                    }),
                lines: section
                    .lines
                    .iter()
                    .map(|line| {
                        if section.kind == SectionKind::Tab {
                            m.tab_line(line)
                        } else {
                            m.line(line)
                        }
                    })
                    .collect(),
            }
        })
        .collect();
    MeasuredSong { header, sections }
}

struct Measurer<'a, M> {
    measure: &'a M,
    metrics: &'a Metrics,
    cache: HashMap<Role, HashMap<String, f32>>,
}

impl<M: Measure> Measurer<'_, M> {
    fn em(&mut self, text: &str, role: Role) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let cache = self.cache.entry(role).or_default();
        if let Some(&width) = cache.get(text) {
            return width;
        }
        let style = self.metrics.style(role);
        let body = self.metrics.reference_size;
        let width = self
            .measure
            .width(text, style.size * body, style.weight, style.face)
            / body;
        cache.insert(text.to_owned(), width);
        width
    }

    fn run(&mut self, role: Role, text: &str, x: f32) -> Run {
        Run {
            role,
            width: self.em(text, role),
            text: text.to_owned(),
            x,
        }
    }

    fn tab_line(&mut self, line: &Line) -> MeasuredLine {
        let run = self.run(Role::Tab, &line.lyrics(), 0.0);
        let width = run.width;
        MeasuredLine {
            above: vec![run.clone()],
            above_width: width,
            inline: vec![run],
            inline_width: width,
        }
    }

    fn line(&mut self, line: &Line) -> MeasuredLine {
        let gap = self.metrics.chord_gap;
        let mut above = Vec::new();
        let (mut lyric_x, mut chord_x) = (0.0_f32, 0.0_f32);
        let mut above_width = 0.0_f32;
        for segment in &line.segments {
            if let Some(chord) = &segment.chord {
                chord_x = chord_x.max(lyric_x);
                let run = self.run(Role::Chord, chord, chord_x);
                chord_x += run.width + gap;
                above_width = above_width.max(run.x + run.width);
                above.push(run);
            }
            if !segment.lyric.is_empty() {
                let run = self.run(Role::Lyric, &segment.lyric, lyric_x);
                lyric_x += run.width;
                above_width = above_width.max(lyric_x);
                above.push(run);
            }
            // A chord wider than its syllable pushes the next lyric past the
            // chord and its gap, which is exactly where the next chord can
            // start, so every chord still sits over the syllable it names.
            lyric_x = lyric_x.max(chord_x);
        }

        let inline_gap = self.metrics.inline_chord_gap;
        let mut inline = Vec::new();
        let mut x = 0.0;
        for segment in &line.segments {
            if let Some(chord) = &segment.chord {
                let run = self.run(Role::InlineChord, chord, x);
                x += run.width + inline_gap;
                inline.push(run);
            }
            if !segment.lyric.is_empty() {
                let run = self.run(Role::Lyric, &segment.lyric, x);
                x += run.width;
                inline.push(run);
            }
        }
        MeasuredLine {
            above,
            above_width,
            inline,
            inline_width: x,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Half an em per character, counting calls.
    struct Fixed(Cell<usize>);
    impl Measure for Fixed {
        fn width(&self, text: &str, size_pt: f32, _: u16, _: Face) -> f32 {
            self.0.set(self.0.get() + 1);
            text.chars().count() as f32 * size_pt * 0.5
        }
    }

    fn metrics() -> Metrics {
        Metrics {
            reference_size: 20.0,
            title_size: 2.0,
            subtitle_size: 1.0,
            label_size: 1.0,
            chord_size: 1.0,
            chord_gap: 0.5,
            inline_chord_size: 0.5,
            inline_chord_gap: 0.25,
        }
    }

    fn measured(source: &str) -> (MeasuredSong, usize) {
        let fixed = Fixed(Cell::new(0));
        let song = cpl_song::parse(source).song;
        (measure_song(&song, &metrics(), &fixed), fixed.0.get())
    }

    #[test]
    fn widths_are_em_and_independent_of_reference_size() {
        let (song, _) = measured("{title: Abc}\n[C]Hello");
        assert_eq!(song.header.title_width, 3.0); // 3 chars × 0.5 × 2 em
        let line = &song.sections[0].lines[0];
        assert_eq!(
            line.above[0],
            Run {
                role: Role::Chord,
                text: "C".into(),
                x: 0.0,
                width: 0.5
            }
        );
        assert_eq!(line.above[1].x, 0.0);
        assert_eq!(line.above_width, 2.5);
    }

    #[test]
    fn chord_overhang_pushes_the_next_lyric_right() {
        // "Cmaj7" is 2.5 em over a 0.5 em syllable, so "bc" moves past it.
        let (song, _) = measured("[Cmaj7]a[G]bc");
        let runs = &song.sections[0].lines[0].above;
        let next_lyric = runs.iter().find(|r| r.text == "bc").unwrap();
        let next_chord = runs.iter().find(|r| r.text == "G").unwrap();
        assert_eq!(next_chord.x, 3.0, "chords keep a gap between them");
        assert_eq!(
            next_lyric.x, next_chord.x,
            "the chord stays over its syllable"
        );
    }

    #[test]
    fn every_chord_starts_at_its_own_lyric() {
        let (song, _) = measured("[Cmaj7]I [G]love [F#m7b5]a [D]song [E]");
        let runs = &song.sections[0].lines[0].above;
        for pair in runs.windows(2) {
            if pair[0].role == Role::Chord && pair[1].role == Role::Lyric {
                assert_eq!(
                    pair[0].x, pair[1].x,
                    "{} over {:?}",
                    pair[0].text, pair[1].text
                );
            }
            assert!(pair[1].x >= pair[0].x, "runs move left to right");
        }
    }

    #[test]
    fn line_width_is_the_extent_of_its_runs() {
        let (song, _) = measured("[C]Hello [Gsus4]wo[Am]rld and [F]more\n[D]x[E]y[F#m7b5]");
        for line in &song.sections[0].lines {
            let extent = |runs: &[Run]| runs.iter().map(|r| r.x + r.width).fold(0.0, f32::max);
            assert_eq!(line.above_width, extent(&line.above));
            // Inline width may include one trailing chord gap, never less than the runs.
            assert!(line.inline_width >= extent(&line.inline));
            let mut x = 0.0;
            for run in &line.inline {
                assert!(run.x >= x, "inline runs never overlap");
                x = run.x + run.width;
            }
        }
    }

    #[test]
    fn repeated_strings_are_measured_once_per_role() {
        let (_, calls) = measured("[C]la[C]la[C]la\n[C]la[C]la");
        // "C" as chord and as inline chord, "la" as lyric, then title "Untitled".
        assert_eq!(calls, 4);
    }

    #[test]
    fn tab_lines_are_one_monospaced_run_and_repeats_are_marked() {
        let (song, _) = measured(
            "{start_of_tab}\ne|--3--|\n{end_of_tab}\n{soc}\n[C]Echo\n{eoc}\n{soc}\n[C]Echo\n{eoc}",
        );
        let tab = &song.sections[0].lines[0];
        assert_eq!(tab.above.len(), 1);
        assert_eq!(tab.above[0].role, Role::Tab);
        assert_eq!(tab.above, tab.inline);
        assert_eq!(song.sections[0].label, "TAB");
        assert!(!song.sections[1].repeats_earlier);
        assert!(song.sections[2].repeats_earlier);
    }
}
