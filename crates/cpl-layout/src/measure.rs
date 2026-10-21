//! Measures a song once into em-unit runs. This is the only layout stage that
//! calls the OS; fitting and placement are arithmetic over these numbers.

use cpl_song::{Line, LineKind, SectionKind, Song};
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
    /// A visible `{comment}` line: lyric-sized, in the soft ink.
    Comment,
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
            Role::Comment => (1.0, 400, Face::System),
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

/// A word of lyric with the whitespace that follows it, measured. A row may
/// break after a token that ends in whitespace, never inside one.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub text: String,
    pub width: f32,
    /// A `Role::Tab` or `Role::Comment` line is one token in that role.
    pub role: Role,
}

/// A measured chord with its width in both chord layouts.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredChord {
    pub text: String,
    pub above_width: f32,
    pub inline_width: f32,
}

/// A song segment, measured: an optional chord and the words it sits over.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredSegment {
    pub chord: Option<MeasuredChord>,
    pub tokens: Vec<Token>,
}

/// One row of a flowed line: its runs and its extent, both in em.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub runs: Vec<Run>,
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredLine {
    /// The source segments with every word measured; `rows` flows them.
    pub segments: Vec<MeasuredSegment>,
    /// Chords on their own row above the lyric, overhang pushing lyrics right.
    pub above: Vec<Run>,
    pub above_width: f32,
    /// Small chords set inline before their lyric segment (ladder step 9).
    pub inline: Vec<Run>,
    pub inline_width: f32,
    /// A chord progression with no sung words, such as an intro or turnaround.
    pub chord_only: bool,
    chord_gap: f32,
    inline_chord_gap: f32,
}

impl MeasuredLine {
    pub fn runs(&self, inline: bool) -> &[Run] {
        if inline && !self.chord_only {
            &self.inline
        } else {
            &self.above
        }
    }

    pub fn width(&self, inline: bool) -> f32 {
        if inline && !self.chord_only {
            self.inline_width
        } else {
            self.above_width
        }
    }

    /// Flows the line into rows no wider than `max_width` em where a word
    /// boundary allows, continuation rows starting at `indent`. A chord
    /// stays over the first word of its segment, so wrapping never moves a
    /// chord away from its syllable. One row of unlimited width is the
    /// unwrapped line, which is how `above` and `inline` are produced; a
    /// single token wider than `max_width` stays whole, and the caller sees
    /// that in the row's width.
    pub fn rows(&self, inline: bool, max_width: f32, indent: f32) -> Vec<Row> {
        let inline = inline && !self.chord_only;
        let gap = if inline {
            self.inline_chord_gap
        } else {
            self.chord_gap
        };
        let mut rows = Vec::new();
        let mut row = RowBuilder::new(0.0, inline, gap);
        for (index, segment) in self.segments.iter().enumerate() {
            let mut chord = segment.chord.as_ref();
            if segment.tokens.is_empty() {
                if let Some(chord) = chord {
                    if row.exceeds(Some(chord), None, max_width) && row.can_break {
                        rows.push(row.finish());
                        row = RowBuilder::new(indent, inline, gap);
                    }
                    row.push(Some(chord), None, index, true);
                }
                continue;
            }
            let last = segment.tokens.len() - 1;
            for (position, token) in segment.tokens.iter().enumerate() {
                if row.exceeds(chord, Some(token), max_width) && row.can_break {
                    rows.push(row.finish());
                    row = RowBuilder::new(indent, inline, gap);
                }
                row.push(chord, Some(token), index, position == last);
                chord = None;
            }
        }
        rows.push(row.finish());
        rows
    }
}

/// Lays runs along one row with the chord-over-syllable rule: a chord wider
/// than its syllable pushes the next lyric past the chord and its gap, which
/// is exactly where the next chord can start.
struct RowBuilder {
    inline: bool,
    gap: f32,
    runs: Vec<Run>,
    lyric_x: f32,
    chord_x: f32,
    width: f32,
    /// Whether the last token ended in whitespace, so a break may follow.
    can_break: bool,
    /// The segment the last lyric run belongs to, so consecutive words of
    /// one segment merge into a single run.
    last_segment: Option<usize>,
}

impl RowBuilder {
    fn new(indent: f32, inline: bool, gap: f32) -> Self {
        Self {
            inline,
            gap,
            runs: Vec::new(),
            lyric_x: indent,
            chord_x: indent,
            width: indent,
            can_break: false,
            last_segment: None,
        }
    }

    fn chord_width(&self, chord: &MeasuredChord) -> f32 {
        if self.inline {
            chord.inline_width
        } else {
            chord.above_width
        }
    }

    /// The row's extent if `chord` and `token` were appended.
    fn exceeds(&self, chord: Option<&MeasuredChord>, token: Option<&Token>, max: f32) -> bool {
        let mut extent = self.width;
        if self.inline {
            let mut x = self.lyric_x;
            if let Some(chord) = chord {
                x += self.chord_width(chord) + self.gap;
                extent = extent.max(x);
            }
            if let Some(token) = token {
                extent = extent.max(x + token.width);
            }
        } else {
            if let Some(chord) = chord {
                let chord_x = self.chord_x.max(self.lyric_x);
                extent = extent.max(chord_x + self.chord_width(chord));
            }
            if let Some(token) = token {
                extent = extent.max(self.lyric_x + token.width);
            }
        }
        extent > max + 1e-4
    }

    /// Appends a chord and/or a token of `segment`. `segment_end` applies the
    /// overhang rule once the whole segment is down, so the words of one
    /// segment stay contiguous under a chord wider than its first word.
    fn push(
        &mut self,
        chord: Option<&MeasuredChord>,
        token: Option<&Token>,
        segment: usize,
        segment_end: bool,
    ) {
        if self.inline {
            if let Some(chord) = chord {
                let width = chord.inline_width;
                self.runs.push(Run {
                    role: Role::InlineChord,
                    text: chord.text.clone(),
                    x: self.lyric_x,
                    width,
                });
                self.lyric_x += width + self.gap;
                self.width = self.width.max(self.lyric_x);
                self.last_segment = None;
            }
        } else if let Some(chord) = chord {
            self.chord_x = self.chord_x.max(self.lyric_x);
            self.runs.push(Run {
                role: Role::Chord,
                text: chord.text.clone(),
                x: self.chord_x,
                width: chord.above_width,
            });
            self.chord_x += chord.above_width + self.gap;
            self.width = self.width.max(self.chord_x - self.gap);
            self.last_segment = None;
        }
        if let Some(token) = token {
            let merged = self.last_segment == Some(segment)
                && self.runs.last().is_some_and(|run| run.role == token.role);
            if merged {
                let run = self.runs.last_mut().unwrap();
                run.text.push_str(&token.text);
                run.width += token.width;
            } else {
                self.runs.push(Run {
                    role: token.role,
                    text: token.text.clone(),
                    x: self.lyric_x,
                    width: token.width,
                });
            }
            self.lyric_x += token.width;
            self.width = self.width.max(self.lyric_x);
            self.can_break = token.text.ends_with(char::is_whitespace);
            self.last_segment = Some(segment);
        }
        if !self.inline && segment_end {
            self.lyric_x = self.lyric_x.max(self.chord_x);
        }
    }

    fn finish(self) -> Row {
        let width = if self.inline {
            self.lyric_x
        } else {
            self.runs
                .iter()
                .map(|run| run.x + run.width)
                .fold(0.0, f32::max)
        };
        Row {
            runs: self.runs,
            width,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredSection {
    pub kind: SectionKind,
    /// Upper-cased, as drawn.
    pub label: String,
    pub label_width: f32,
    /// A `{chorus}` recall, or a chorus retyped with the same lines as an
    /// earlier one (ladder step 8).
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
                repeats_earlier: section.recall_of.is_some()
                    || section.kind == SectionKind::Chorus
                        && song.sections[..index].iter().any(|earlier| {
                            earlier.kind == SectionKind::Chorus && earlier.lines == section.lines
                        }),
                lines: section
                    .lines
                    .iter()
                    .map(|line| {
                        if section.kind == SectionKind::Tab {
                            m.whole_line(line, Role::Tab)
                        } else if line.kind == LineKind::Comment {
                            m.whole_line(line, Role::Comment)
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

    /// Tablature and comments: one token, the same in both chord layouts.
    /// A tab row never wraps; a comment wraps by its words.
    fn whole_line(&mut self, line: &Line, role: Role) -> MeasuredLine {
        let text = line.lyrics();
        let tokens = if role == Role::Tab {
            vec![Token {
                width: self.em(&text, role),
                text,
                role,
            }]
        } else {
            self.tokens(&text, role)
        };
        self.flowed(
            vec![MeasuredSegment {
                chord: None,
                tokens,
            }],
            false,
        )
    }

    /// Words with their trailing whitespace, each measured on its own.
    fn tokens(&mut self, text: &str, role: Role) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut start = 0;
        let bytes: Vec<(usize, char)> = text.char_indices().collect();
        let mut index = 0;
        while index < bytes.len() {
            // a word, then its whitespace
            while index < bytes.len() && !bytes[index].1.is_whitespace() {
                index += 1;
            }
            while index < bytes.len() && bytes[index].1.is_whitespace() {
                index += 1;
            }
            let end = bytes.get(index).map_or(text.len(), |(offset, _)| *offset);
            let word = &text[start..end];
            tokens.push(Token {
                width: self.em(word, role),
                text: word.to_owned(),
                role,
            });
            start = end;
        }
        tokens
    }

    fn line(&mut self, line: &Line) -> MeasuredLine {
        let chord_only = line.segments.iter().any(|segment| segment.chord.is_some())
            && line
                .segments
                .iter()
                .all(|segment| segment.lyric.trim().is_empty());
        let segments = line
            .segments
            .iter()
            .map(|segment| MeasuredSegment {
                chord: segment.chord.as_ref().map(|chord| MeasuredChord {
                    above_width: self.em(chord, Role::Chord),
                    inline_width: self.em(chord, Role::InlineChord),
                    text: chord.clone(),
                }),
                tokens: self.tokens(&segment.lyric, Role::Lyric),
            })
            .collect();
        self.flowed(segments, chord_only)
    }

    /// The unwrapped line in both layouts is the one-row flow of its segments.
    fn flowed(&self, segments: Vec<MeasuredSegment>, chord_only: bool) -> MeasuredLine {
        let mut line = MeasuredLine {
            segments,
            above: Vec::new(),
            above_width: 0.0,
            inline: Vec::new(),
            inline_width: 0.0,
            chord_only,
            chord_gap: self.metrics.chord_gap,
            inline_chord_gap: self.metrics.inline_chord_gap,
        };
        let above = line.rows(false, f32::INFINITY, 0.0).remove(0);
        line.above = above.runs;
        line.above_width = above.width;
        let mut inline = line.rows(true, f32::INFINITY, 0.0).remove(0);
        if chord_only {
            // Chord-only lines keep their full chord row at the inline step.
            inline = line.rows(false, f32::INFINITY, 0.0).remove(0);
        }
        line.inline = inline.runs;
        line.inline_width = inline.width;
        line
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
    fn a_wide_line_wraps_at_a_word_boundary_with_its_chords_following() {
        // Fixed: 0.5 em per character. "Hello there " is 6 em, "my " 1.5 em.
        let (song, _) = measured("[C]Hello there [G]my [Am]friend of old");
        let line = &song.sections[0].lines[0];
        assert_eq!(line.rows(false, f32::INFINITY, 0.0).len(), 1);
        let rows = line.rows(false, 8.0, 1.0);
        assert_eq!(rows.len(), 2, "{rows:?}");
        let first: Vec<&str> = rows[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(first, ["C", "Hello there ", "G", "my "]);
        let second: Vec<&str> = rows[1].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(second, ["Am", "friend of old"]);
        assert_eq!(rows[1].runs[0].x, 1.0, "continuation rows are indented");
        assert_eq!(rows[1].runs[1].x, 1.0, "the chord still sits over its word");
        assert!(rows[0].width <= 8.0 && rows[1].width <= 8.0);
    }

    #[test]
    fn wrapping_never_breaks_inside_a_word_or_after_a_syllable_split() {
        let (song, _) = measured("[D]New Or[F]leans is [G]far");
        let line = &song.sections[0].lines[0];
        // 3 em fits "New " and nothing else; "Or" + "leans" must stay together.
        let rows = line.rows(false, 3.0, 0.0);
        let texts: Vec<Vec<&str>> = rows
            .iter()
            .map(|row| row.runs.iter().map(|r| r.text.as_str()).collect())
            .collect();
        // "New Or" merges into one run; "Or" cannot leave "leans" behind, so
        // the F chord's segment overruns the limit rather than breaking.
        assert_eq!(
            texts,
            [vec!["D", "New Or", "F", "leans "], vec!["is ", "G", "far"]]
        );
        // An unbreakable token wider than the limit stays whole and reports it.
        let (song, _) = measured("[C]Supercalifragilistic");
        let rows = song.sections[0].lines[0].rows(false, 2.0, 0.0);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].width > 2.0);
    }

    #[test]
    fn inline_rows_wrap_too_and_tab_rows_never_do() {
        let (song, _) = measured("[C]one two [G]three four");
        let line = &song.sections[0].lines[0];
        let rows = line.rows(true, 4.5, 0.5);
        assert!(rows.len() >= 2);
        assert!(rows.iter().all(|row| row.width <= 4.5 + 1e-4), "{rows:?}");
        let (song, _) = measured("{sot}\ne|--3--|--5--|--7--|\n{eot}");
        let tab = &song.sections[0].lines[0];
        assert_eq!(tab.rows(false, 1.0, 0.0).len(), 1);
    }

    #[test]
    fn chord_only_line_retains_full_chords_at_the_inline_reduction_step() {
        let (song, _) = measured("[G] [Am] [C] [G]\n[C]Hello");
        let progression = &song.sections[0].lines[0];
        let lyric = &song.sections[0].lines[1];
        assert!(progression.chord_only);
        assert!(!lyric.chord_only);
        assert_eq!(progression.runs(true), progression.above);
        assert_eq!(progression.width(true), progression.above_width);
    }

    #[test]
    fn comments_are_one_run_and_recalls_count_as_repeats() {
        let (song, _) = measured(
            "{soc: Chorus}\n[C]Sing\n{c: Repeat x2}\n{eoc}\n{sov}\n[G]Verse\n{eov}\n{chorus}",
        );
        let comment = &song.sections[0].lines[1];
        assert_eq!(comment.above.len(), 1);
        assert_eq!(comment.above[0].role, Role::Comment);
        assert_eq!(comment.above[0].text, "Repeat x2");
        assert_eq!(comment.above, comment.inline);
        assert!(!song.sections[0].repeats_earlier);
        assert!(song.sections[2].repeats_earlier);
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
