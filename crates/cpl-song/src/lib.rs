//! Platform-independent song model and a ChordPro 6 parser, growing toward
//! conformance. `~/src/chordpro-library` holds the working spec it follows.

pub mod chords;
pub mod substitute;
pub mod transpose;

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Song {
    pub metadata: BTreeMap<String, Vec<String>>,
    pub sections: Vec<Section>,
    /// `{define}` voicings by chord name; they win over the built-in table.
    pub defines: BTreeMap<String, chords::Shape>,
    /// The viewer's live transpose already applied to the chords, in
    /// semitones; a `{transpose}` in the file is not counted here.
    pub transpose: i32,
}

impl Song {
    pub fn first_meta(&self, name: &str) -> Option<&str> {
        self.metadata.get(name)?.first().map(String::as_str)
    }

    /// Every chord the song uses, in first-use order, each once.
    pub fn chord_names(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        for segment in self
            .sections
            .iter()
            .filter(|section| section.kind != SectionKind::Tab)
            .flat_map(|section| &section.lines)
            .flat_map(|line| &line.segments)
        {
            // A grid cell may hold several chords, `C~G`.
            for chord in segment.chord.iter().flat_map(|cell| cell.split('~')) {
                if !seen.contains(&chord) {
                    seen.push(chord);
                }
            }
        }
        seen
    }

    /// A copy with every chord and key shifted by `semitones` more, spelled
    /// by the fixed rule. Tablature is verbatim and stays as written;
    /// `{define}` voicings keep their names, so a shifted chord falls back to
    /// the built-in table unless the song defines it too.
    pub fn transposed(&self, semitones: i32) -> Song {
        let mut song = self.clone();
        if semitones == 0 {
            return song;
        }
        song.transpose += semitones;
        let shift = |name: &str| shift_cell(name, semitones, transpose::Spelling::Fixed);
        for section in song
            .sections
            .iter_mut()
            .filter(|section| section.kind != SectionKind::Tab)
        {
            for segment in section.lines.iter_mut().flat_map(|line| &mut line.segments) {
                if let Some(chord) = &mut segment.chord {
                    *chord = shift(chord);
                }
            }
        }
        for name in ["key", "key_actual"] {
            if let Some(keys) = song.metadata.get_mut(name) {
                for key in keys {
                    *key = shift(key);
                }
            }
        }
        song
    }

    /// The fingering to draw for `name`: the song's `{define}`, else the
    /// built-in table's.
    pub fn shape(&self, name: &str) -> Option<chords::Shape> {
        self.defines
            .get(name)
            .cloned()
            .or_else(|| chords::lookup(name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub label: String,
    pub lines: Vec<Line>,
    /// A `{chorus}` recall: the index of the chorus section it repeats. Its
    /// lines are a copy of that chorus, so a renderer needs no lookup.
    pub recall_of: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Verse,
    Chorus,
    Bridge,
    Tab,
    /// A chord grid: each line is its cells in order, a chord cell as a
    /// chord segment and a bar, `.` or other symbol as a lyric segment.
    Grid,
    /// Text set as a block; its lines are parsed like a verse's.
    Textblock,
    /// Notation handed to another program, kept verbatim and not drawn.
    Embedded(Notation),
    /// A stanza no environment encloses; blank lines separate these.
    Unlabelled,
}

impl SectionKind {
    /// Kept as written: tablature and embedded notation.
    pub fn verbatim(self) -> bool {
        matches!(self, SectionKind::Tab | SectionKind::Embedded(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notation {
    Abc,
    Lilypond,
    Svg,
}

impl Notation {
    /// The environment's name in `{start_of_…}`.
    pub fn name(self) -> &'static str {
        match self {
            Notation::Abc => "abc",
            Notation::Lilypond => "ly",
            Notation::Svg => "svg",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineKind {
    #[default]
    Lyrics,
    /// A visible `{comment}` (or `{ci}`, `{cb}`, `{highlight}`); its text is
    /// the single segment's lyric.
    Comment,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    pub segments: Vec<Segment>,
}

impl Line {
    pub fn lyrics(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.lyric.as_str())
            .collect()
    }

    fn text(kind: LineKind, text: String) -> Self {
        Self {
            kind,
            segments: vec![Segment {
                chord: None,
                annotation: None,
                lyric: text,
            }],
        }
    }

    /// A grid line's cells: each chord, bar and symbol, in order.
    pub fn cells(&self) -> impl Iterator<Item = &str> {
        self.segments
            .iter()
            .map(|segment| segment.chord.as_deref().unwrap_or(&segment.lyric))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub chord: Option<String>,
    /// A `[*text]` annotation, drawn where a chord would be: not a chord, so
    /// never transposed and never given a diagram. A segment carries a chord
    /// or an annotation, not both.
    pub annotation: Option<String>,
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

const METADATA: &[&str] = &[
    "title",
    "subtitle",
    "artist",
    "sortartist",
    "composer",
    "lyricist",
    "arranger",
    "copyright",
    "album",
    "year",
    "key",
    "time",
    "tempo",
    "duration",
    "capo",
    "sorttitle",
    "tag",
];

/// The instrument this viewer draws diagrams for, which a directive's
/// `-selector` is matched against first.
pub const INSTRUMENT: &str = "guitar";

/// One song's text within a file, and how many lines of the file precede it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Part<'a> {
    pub offset: usize,
    pub text: &'a str,
}

/// The songs a file holds, split at each `{new_song}` (or `{ns}`) outside
/// tablature and embedded notation. A file without one is a single part
/// whose text is the whole file, so its fingerprint is the file's.
pub fn split(source: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let (mut start, mut start_line, mut at) = (0, 0, 0);
    let mut verbatim: Option<String> = None;
    for (index, raw) in source.split_inclusive('\n').enumerate() {
        let next = at + raw.len();
        let owned = directive(raw.trim_start_matches('\u{feff}').trim()).map(|(name, _)| name);
        let name = owned.as_deref().map(canonical);
        if let Some(end) = &verbatim {
            if name == Some(end.as_str()) {
                verbatim = None;
            }
        } else if let Some(kind) = name
            .and_then(|name| name.strip_prefix("start_of_"))
            .and_then(environment)
            .filter(|kind| kind.verbatim())
        {
            verbatim = Some(end_of(kind));
        } else if name == Some("new_song") {
            parts.push(Part {
                offset: start_line,
                text: &source[start..at],
            });
            (start, start_line) = (next, index + 1);
        }
        at = next;
    }
    parts.push(Part {
        offset: start_line,
        text: &source[start..],
    });
    // Before a leading `{new_song}`, or between two, there is no song.
    let has_content = |text: &str| {
        text.lines().any(|line| {
            let line = line.trim_start_matches('\u{feff}').trim();
            !line.is_empty() && !line.starts_with('#')
        })
    };
    let mut songs: Vec<Part> = parts.into_iter().filter(|p| has_content(p.text)).collect();
    if songs.is_empty() {
        songs.push(Part {
            offset: 0,
            text: source,
        });
    }
    songs
}

/// Every song in a file, each parsed on its own; diagnostics count lines
/// from the top of the file.
pub fn parse_all(source: &str) -> Vec<ParseResult> {
    split(source)
        .into_iter()
        .map(|part| {
            let mut result = parse(part.text);
            for diagnostic in &mut result.diagnostics {
                diagnostic.line += part.offset;
            }
            result
        })
        .collect()
}

/// Parses one song: metadata, environments, comments, chorus recall,
/// bracketed chords and annotations, selectors and `%{…}` substitution.
/// Formatting directives are accepted and ignored by design; anything else
/// unsupported is diagnosed rather than silently discarded. A file of
/// several songs is read with [`parse_all`].
pub fn parse(source: &str) -> ParseResult {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut parser = Parser::default();
    let mut last = 0;
    for (index, raw) in source.lines().enumerate() {
        last = index + 1;
        parser.line(last, raw.trim_end_matches('\r'));
    }
    parser.finish(last)
}

#[derive(Default)]
struct Parser {
    song: Song,
    diagnostics: Vec<Diagnostic>,
    active: Option<Section>,
    /// The `{transpose}` in force: chords after it are shifted as parsed.
    transpose: (i32, transpose::Spelling),
    /// The end directive awaited after an environment whose selector did
    /// not match; everything until it is skipped.
    skipping: Option<String>,
}

impl Parser {
    fn diagnose(&mut self, line: usize, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            line,
            message: message.into(),
        });
    }

    fn active_kind(&self) -> Option<SectionKind> {
        self.active.as_ref().map(|section| section.kind)
    }

    /// The open section, if an environment directive opened it.
    fn explicit(&self) -> Option<SectionKind> {
        self.active_kind()
            .filter(|&kind| kind != SectionKind::Unlabelled)
    }

    fn close(&mut self) {
        if let Some(section) = self.active.take() {
            self.song.sections.push(section);
        }
    }

    fn push_line(&mut self, line: Line) {
        self.active
            .get_or_insert_with(|| Section {
                kind: SectionKind::Unlabelled,
                label: String::new(),
                lines: Vec::new(),
                recall_of: None,
            })
            .lines
            .push(line);
    }

    fn line(&mut self, number: usize, text: &str) {
        let trimmed = text.trim();
        if let Some(end) = &self.skipping {
            if directive(trimmed).is_some_and(|(name, _)| canonical(&name) == end) {
                self.skipping = None;
            }
            return;
        }
        if let Some(kind) = self.active_kind().filter(|kind| kind.verbatim()) {
            // Tablature and notation are verbatim: blank lines, `#` and
            // brackets included.
            match directive(trimmed) {
                Some((name, _)) if canonical(&name) == end_of(kind) => self.close(),
                _ => self.push_line(Line::text(LineKind::Lyrics, expand_tabs(text))),
            }
            return;
        }
        if trimmed.is_empty() {
            // A blank line ends a stanza that no environment encloses.
            if self.active_kind() == Some(SectionKind::Unlabelled) {
                self.close();
            }
            return;
        }
        if trimmed.starts_with('#') {
            return;
        }
        if let Some((name, value)) = directive(trimmed) {
            let (name, selector) = match name.split_once('-') {
                Some((name, selector)) => (name, Some(selector)),
                None => (name.as_str(), None),
            };
            let name = canonical(name);
            if let Some(selector) = selector
                && !self.selected(selector)
            {
                if let Some(kind) = name.strip_prefix("start_of_").and_then(environment) {
                    self.skipping = Some(end_of(kind));
                }
                return;
            }
            let value = substitute::substitute(value, &self.song.metadata);
            self.directive(number, name, &value);
            return;
        }
        let text = substitute::substitute(text, &self.song.metadata);
        let line = if self.active_kind() == Some(SectionKind::Grid) {
            grid_line(&text, self.transpose)
        } else {
            parse_line(&text, number, self.transpose, &mut self.diagnostics)
        };
        self.push_line(line);
    }

    /// Whether a directive's `-selector` (`!` negates) applies: the
    /// instrument, else a metadata item that is set and not false.
    fn selected(&self, selector: &str) -> bool {
        let (selector, negated) = match selector.strip_suffix('!') {
            Some(selector) => (selector, true),
            None => (selector, false),
        };
        let truthy = |value: &String| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "null"
            )
        };
        let hit = selector == INSTRUMENT
            || self
                .song
                .metadata
                .get(selector)
                .and_then(|values| values.first())
                .is_some_and(truthy);
        hit != negated
    }

    fn directive(&mut self, number: usize, name: &str, value: &str) {
        if let Some(kind) = name.strip_prefix("start_of_").and_then(environment) {
            // A grid's bare argument is its shape, and notation's is not a
            // label either; only `label="…"` names them.
            let label = match kind {
                SectionKind::Grid | SectionKind::Embedded(_) => {
                    attribute(value, "label").unwrap_or("")
                }
                _ => label(value),
            };
            self.start(number, kind, label);
            if let SectionKind::Embedded(notation) = kind {
                self.diagnose(
                    number,
                    format!("notation not drawn yet: {}", notation.name()),
                );
            }
            return;
        }
        if let Some(kind) = name.strip_prefix("end_of_").and_then(environment) {
            self.end(number, kind, name);
            return;
        }
        match name {
            "new_song" => {
                // Implicit at the top of a song; a second song needs parse_all.
                if self.song != Song::default() || self.active.is_some() {
                    self.diagnose(
                        number,
                        "new_song inside one song: read the file with parse_all",
                    );
                }
            }
            "chorus" => self.recall(number, label(value)),
            "define" => match chords::parse_define(value) {
                Some((name, shape)) => {
                    self.song.defines.insert(name, shape);
                }
                None => self.diagnose(number, "define needs a chord name and frets"),
            },
            "transpose" => match transpose::parse_directive(value) {
                Some(shift) => {
                    self.transpose = shift;
                    // The key the player now reads; `key` stays as written.
                    if let Some(key) = self.song.first_meta("key") {
                        let actual = transpose::chord(key, shift.0, shift.1);
                        self.song.metadata.insert("key_actual".into(), vec![actual]);
                    }
                }
                None => self.diagnose(number, format!("transpose needs semitones, not {value:?}")),
            },
            "comment" | "comment_italic" | "comment_box" | "highlight" => {
                if value.is_empty() {
                    self.diagnose(number, format!("empty {name}"));
                } else {
                    self.push_line(Line::text(LineKind::Comment, value.into()));
                }
            }
            "meta" => match value.split_once(char::is_whitespace) {
                Some((key, rest)) if !key.is_empty() => self
                    .song
                    .metadata
                    .entry(key.to_ascii_lowercase())
                    .or_default()
                    .push(rest.trim().into()),
                _ => self.diagnose(number, "meta needs a name and a value"),
            },
            _ if METADATA.contains(&name) => self
                .song
                .metadata
                .entry(name.into())
                .or_default()
                .push(value.into()),
            // Private extensions and formatting are not ours to apply.
            _ if name.starts_with("x_") || is_formatting(name) => {}
            "new_page" | "new_physical_page" | "column_break" | "columns" => {
                self.diagnose(number, format!("layout hint not honoured yet: {name}"));
            }
            _ => self.diagnose(number, format!("unsupported directive: {name}")),
        }
    }

    fn start(&mut self, number: usize, kind: SectionKind, label: &str) {
        if self.explicit().is_some() {
            self.diagnose(number, "section started before previous section ended");
        }
        self.close();
        self.active = Some(Section {
            kind,
            label: label.into(),
            lines: Vec::new(),
            recall_of: None,
        });
    }

    fn end(&mut self, number: usize, kind: SectionKind, name: &str) {
        match self.explicit() {
            Some(open) if open == kind => self.close(),
            Some(open) => {
                self.diagnose(number, format!("{name} closes a {open:?} section"));
                self.close();
            }
            None => self.diagnose(number, format!("{name} without a start")),
        }
    }

    fn recall(&mut self, number: usize, label: &str) {
        if self.explicit().is_some() {
            self.diagnose(number, "chorus recall inside another section");
        }
        self.close();
        let Some(index) = self
            .song
            .sections
            .iter()
            .rposition(|section| section.kind == SectionKind::Chorus)
        else {
            self.diagnose(number, "chorus recall before any chorus");
            return;
        };
        let original = &self.song.sections[index];
        let recalled = Section {
            kind: SectionKind::Chorus,
            label: if label.is_empty() {
                original.label.clone()
            } else {
                label.into()
            },
            lines: original.lines.clone(),
            // A recall of a recall still points at the chorus as written.
            recall_of: Some(original.recall_of.unwrap_or(index)),
        };
        self.song.sections.push(recalled);
    }

    fn finish(mut self, last: usize) -> ParseResult {
        if let Some(kind) = self.explicit() {
            self.diagnose(last, format!("{kind:?} section not closed"));
        }
        self.close();
        ParseResult {
            song: self.song,
            diagnostics: self.diagnostics,
        }
    }
}

/// Splits `{name: value}`, `{name value}` and `{name:value}` into a name and
/// an argument; the separator is a colon and/or whitespace.
fn directive(trimmed: &str) -> Option<(String, &str)> {
    let body = trimmed.strip_prefix('{')?.strip_suffix('}')?.trim();
    let end = body
        .find(|c: char| c == ':' || c.is_whitespace())
        .unwrap_or(body.len());
    let (name, rest) = body.split_at(end);
    let rest = rest.trim_start();
    let value = rest.strip_prefix(':').unwrap_or(rest).trim();
    Some((name.to_ascii_lowercase(), value))
}

fn canonical(name: &str) -> &str {
    match name {
        "t" => "title",
        "st" => "subtitle",
        "a" => "artist",
        "c" => "comment",
        "ci" => "comment_italic",
        "cb" => "comment_box",
        "sov" => "start_of_verse",
        "eov" => "end_of_verse",
        "soc" => "start_of_chorus",
        "eoc" => "end_of_chorus",
        "sob" => "start_of_bridge",
        "eob" => "end_of_bridge",
        "sot" => "start_of_tab",
        "eot" => "end_of_tab",
        "ns" => "new_song",
        "sog" => "start_of_grid",
        "eog" => "end_of_grid",
        "np" => "new_page",
        "npp" => "new_physical_page",
        "colb" => "column_break",
        "col" => "columns",
        other => other,
    }
}

/// The section an environment name opens or closes: `verse` for
/// `{start_of_verse}`, and so on.
fn environment(name: &str) -> Option<SectionKind> {
    Some(match name {
        "verse" => SectionKind::Verse,
        "chorus" => SectionKind::Chorus,
        "bridge" => SectionKind::Bridge,
        "tab" => SectionKind::Tab,
        "grid" => SectionKind::Grid,
        "textblock" => SectionKind::Textblock,
        "abc" => SectionKind::Embedded(Notation::Abc),
        "ly" => SectionKind::Embedded(Notation::Lilypond),
        "svg" => SectionKind::Embedded(Notation::Svg),
        _ => return None,
    })
}

/// The directive that closes an environment.
fn end_of(kind: SectionKind) -> String {
    let name = match kind {
        SectionKind::Verse | SectionKind::Unlabelled => "verse",
        SectionKind::Chorus => "chorus",
        SectionKind::Bridge => "bridge",
        SectionKind::Tab => "tab",
        SectionKind::Grid => "grid",
        SectionKind::Textblock => "textblock",
        SectionKind::Embedded(notation) => notation.name(),
    };
    format!("end_of_{name}")
}

/// A chord shifted by `semitones`; a grid cell's `C~G` shifts each chord.
fn shift_cell(cell: &str, semitones: i32, spelling: transpose::Spelling) -> String {
    cell.split('~')
        .map(|chord| transpose::chord(chord, semitones, spelling))
        .collect::<Vec<_>>()
        .join("~")
}

/// A grid line's cells. Spacing is not significant. Between the first and
/// last bar (`|`, `||`, `|:`, `:|2` …) a `.`, `/` or `%` is a symbol and any
/// other cell a chord; text before the first bar is the row's margin (always a cell,
/// perhaps empty) and text after the last its comment, each one cell kept
/// as written.
fn grid_line(text: &str, (semitones, spelling): (i32, transpose::Spelling)) -> Line {
    let bar = |cell: &str| cell.starts_with(['|', ':']);
    let cells: Vec<&str> = text.split_whitespace().collect();
    let (first, last) = match (
        cells.iter().position(|c| bar(c)),
        cells.iter().rposition(|c| bar(c)),
    ) {
        (Some(first), Some(last)) => (first, last + 1),
        _ => (0, cells.len()),
    };
    let text_cell = |words: &[&str]| Segment {
        chord: None,
        annotation: None,
        lyric: words.join(" "),
    };
    let symbol = |cell: &str| bar(cell) || matches!(cell, "." | "/" | "%" | "%%" | "~");
    let bars = cells[first..last].iter().map(|&cell| Segment {
        chord: (!symbol(cell)).then(|| shift_cell(cell, semitones, spelling)),
        annotation: None,
        lyric: if symbol(cell) {
            cell.into()
        } else {
            String::new()
        },
    });
    Line {
        kind: LineKind::Lyrics,
        // The margin is kept even when empty, so a grid's rows align by
        // column; an empty comment is dropped.
        segments: std::iter::once(text_cell(&cells[..first]))
            .chain(bars)
            .chain((last < cells.len()).then(|| text_cell(&cells[last..])))
            .collect(),
    }
}

/// The `*font`, `*size` and `*colour` families and the other directives that
/// describe appearance, which this viewer owns.
pub fn is_formatting(name: &str) -> bool {
    const FAMILIES: [&str; 9] = [
        "chord", "chorus", "text", "tab", "grid", "label", "title", "toc", "footer",
    ];
    FAMILIES.iter().any(|family| {
        name.strip_prefix(family)
            .is_some_and(|rest| matches!(rest, "font" | "size" | "colour" | "color"))
    }) || matches!(
        name,
        "tf" | "ts"
            | "tc"
            | "cf"
            | "cs"
            | "cc"
            | "titles"
            | "diagrams"
            | "grid"
            | "g"
            | "no_grid"
            | "ng"
            | "pagetype"
    )
}

/// An environment's label: a `label="…"` attribute, or the bare argument.
fn label(value: &str) -> &str {
    if let Some(found) = attribute(value, "label") {
        return found;
    }
    if value.contains("=\"") || value.contains("='") {
        "" // attributes, but none of them a label
    } else {
        value
    }
}

fn attribute<'a>(value: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = value;
    while let Some(at) = rest.find(key) {
        let before = &rest[..at];
        let after = rest[at + key.len()..].trim_start();
        rest = &rest[at + key.len()..];
        if !(before.is_empty() || before.ends_with(char::is_whitespace)) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let inner = &after[1..];
        return inner.find(quote).map(|end| &inner[..end]);
    }
    None
}

/// Tab characters in tablature advance to the next multiple of eight columns.
fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let spaces = 8 - column % 8;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

fn parse_line(
    text: &str,
    number: usize,
    (semitones, spelling): (i32, transpose::Spelling),
    diagnostics: &mut Vec<Diagnostic>,
) -> Line {
    let mut segments = Vec::new();
    let mut remaining = text;
    while let Some(open) = remaining.find('[') {
        if open > 0 {
            segments.push(Segment {
                chord: None,
                annotation: None,
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
        let marker = &after[..close];
        if marker.is_empty() {
            diagnostics.push(Diagnostic {
                line: number,
                message: "empty chord bracket".into(),
            });
        }
        remaining = &after[close + 1..];
        let next = remaining.find('[').unwrap_or(remaining.len());
        let (chord, annotation) = match marker.strip_prefix('*') {
            Some(text) => (None, Some(text.to_owned())),
            None => (Some(transpose::chord(marker, semitones, spelling)), None),
        };
        segments.push(Segment {
            chord,
            annotation,
            lyric: remaining[..next].into(),
        });
        remaining = &remaining[next..];
    }
    if !remaining.is_empty() {
        segments.push(Segment {
            chord: None,
            annotation: None,
            lyric: remaining.into(),
        });
    }
    Line {
        kind: LineKind::Lyrics,
        segments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(result: &ParseResult) -> Vec<&str> {
        result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect()
    }

    fn labels(result: &ParseResult) -> Vec<(SectionKind, &str)> {
        result
            .song
            .sections
            .iter()
            .map(|s| (s.kind, s.label.as_str()))
            .collect()
    }

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
        let result = parse("{title: Test}\n{start_of_part}\n");
        assert_eq!(result.diagnostics[0].line, 2);
        assert_eq!(
            result.diagnostics[0].message,
            "unsupported directive: start_of_part"
        );
    }

    #[test]
    fn new_song_splits_a_file_into_songs() {
        let source = "{title: One}\n[C]First\n{new_song}\n{title: Two}\n{sot}\n{ns}\n{eot}\n[G]Second\n{ns}\n# end\n";
        let parts = split(source);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert_eq!(parts[0].text, "{title: One}\n[C]First\n");
        assert_eq!(parts[1].offset, 3);
        let songs = parse_all(source);
        assert_eq!(songs[0].song.first_meta("title"), Some("One"));
        assert_eq!(songs[1].song.first_meta("title"), Some("Two"));
        assert_eq!(
            songs[1].song.sections[0].lines[0].lyrics(),
            "{ns}",
            "a {{ns}} inside tablature is tablature"
        );
        let unclosed = parse_all("{title: A}\n{ns}\n{title: B}\n{soc}\n[C]x");
        assert_eq!(
            unclosed[1].diagnostics[0].line, 5,
            "lines count from the file's top"
        );

        let single = "\u{feff}{title: Only}\n[C]x\n";
        assert_eq!(
            split(single),
            [Part {
                offset: 0,
                text: single
            }],
            "the whole file, BOM and all"
        );
        let leading = parse_all("{new_song}\n{title: Only}\n");
        assert_eq!(leading.len(), 1);
        assert!(leading[0].diagnostics.is_empty());
        assert_eq!(
            messages(&parse("{title: A}\n{new_song}\n{title: B}")),
            ["new_song inside one song: read the file with parse_all"]
        );
    }

    #[test]
    fn annotations_sit_where_chords_do_but_are_not_chords() {
        let result = parse("{key: C}\n[C]Sing [*Riff x2]along [*]");
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        let line = &result.song.sections[0].lines[0];
        assert_eq!(line.segments[1].chord, None);
        assert_eq!(line.segments[1].annotation.as_deref(), Some("Riff x2"));
        assert_eq!(line.segments[1].lyric, "along ");
        assert_eq!(line.lyrics(), "Sing along ");
        assert_eq!(result.song.chord_names(), ["C"]);
        let up = result.song.transposed(2);
        assert_eq!(
            up.sections[0].lines[0].segments[1].annotation.as_deref(),
            Some("Riff x2")
        );
    }

    #[test]
    fn selectors_keep_what_matches_the_guitar_or_a_set_item() {
        let result = parse(
            "{title: T}\n{year: 0}\n{comment-guitar: G}\n{comment-ukulele: U}\n{comment-guitar!: notG}\n{comment-ukulele!: notU}\n{comment-title: hasTitle}\n{comment-album: hasAlbum}\n{comment-year: year}\n{start_of_verse-ukulele}\n[C]uke\n{end_of_verse}\n{start_of_tab-ukulele}\n{end_of_verse}\n{end_of_tab}\n{start_of_chorus-guitar}\n[G]after\n{end_of_chorus}",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        let lines: Vec<String> = result
            .song
            .sections
            .iter()
            .flat_map(|s| &s.lines)
            .map(Line::lyrics)
            .collect();
        assert_eq!(lines, ["G", "notU", "hasTitle", "after"]);
    }

    #[test]
    fn metadata_substitutes_into_text_as_it_stands_at_that_line() {
        let result = parse(
            "{artist: Ada}\n{title: %{artist} song}\n{c: %{title} by %{artist}}\n{sov: %{artist}}\n[C]Sing %{title} [*%{artist}]\n{eov}\n{album: Later}\n{c: %{album}}",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        assert_eq!(result.song.first_meta("title"), Some("Ada song"));
        let song = &result.song;
        assert_eq!(song.sections[0].lines[0].lyrics(), "Ada song by Ada");
        assert_eq!(song.sections[1].label, "Ada");
        let line = &song.sections[1].lines[0];
        assert_eq!(line.lyrics(), "Sing Ada song ");
        assert_eq!(line.segments[1].annotation.as_deref(), Some("Ada"));
        assert_eq!(song.sections[2].lines[0].lyrics(), "Later");
    }

    #[test]
    fn grids_are_cells_and_their_chords_transpose() {
        let result = parse(
            "{start_of_grid: shape=\"4x2\" label=\"Intro\"}\n|| Am . . . | C~G . . . |\nIn A |:  D  /  /  /  :| x2 [F]\n{end_of_grid}\n{sog 4x4}\n| E |\n{eog}",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        let grid = &result.song.sections[0];
        assert_eq!(
            (grid.kind, grid.label.as_str()),
            (SectionKind::Grid, "Intro")
        );
        let cells: Vec<&str> = grid.lines[1].cells().collect();
        assert_eq!(cells, ["In A", "|:", "D", "/", "/", "/", ":|", "x2 [F]"]);
        assert_eq!(
            grid.lines[1].segments[0].chord, None,
            "the margin is no chord"
        );
        assert_eq!(
            grid.lines[0].cells().collect::<Vec<_>>(),
            [
                "", "||", "Am", ".", ".", ".", "|", "C~G", ".", ".", ".", "|"
            ]
        );
        assert_eq!(
            result.song.sections[1].label, "",
            "a bare argument is the shape"
        );
        assert_eq!(result.song.chord_names(), ["Am", "C", "G", "D", "E"]);
        let up = result.song.transposed(2);
        assert_eq!(
            up.sections[0].lines[0].cells().collect::<Vec<_>>(),
            [
                "", "||", "Bm", ".", ".", ".", "|", "D~A", ".", ".", ".", "|"
            ]
        );
    }

    #[test]
    fn textblocks_read_like_verses_and_notation_is_kept_verbatim() {
        let result = parse(
            "{start_of_textblock: label=\"Note\"}\nSome [C]text\n{end_of_textblock}\n{start_of_abc}\nX:1\n[K:C]\n\n{end_of_abc}\n{start_of_ly}\nc d\n{end_of_ly}\n{start_of_svg}\n<svg/>\n{end_of_svg}",
        );
        let kinds: Vec<_> = result.song.sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [
                SectionKind::Textblock,
                SectionKind::Embedded(Notation::Abc),
                SectionKind::Embedded(Notation::Lilypond),
                SectionKind::Embedded(Notation::Svg),
            ]
        );
        assert_eq!(result.song.sections[0].label, "Note");
        assert_eq!(
            result.song.sections[0].lines[0].segments[1]
                .chord
                .as_deref(),
            Some("C")
        );
        let abc: Vec<String> = result.song.sections[1]
            .lines
            .iter()
            .map(Line::lyrics)
            .collect();
        assert_eq!(abc, ["X:1", "[K:C]", ""]);
        assert_eq!(
            messages(&result),
            [
                "notation not drawn yet: abc",
                "notation not drawn yet: ly",
                "notation not drawn yet: svg"
            ]
        );
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
        let result = parse("{start_of_tab: Intro}\ne|--[3]---  |\n\n# not a remark\n{end_of_tab}");
        assert!(result.diagnostics.is_empty());
        let tab = &result.song.sections[0];
        assert_eq!(tab.kind, SectionKind::Tab);
        assert_eq!(tab.lines[0].lyrics(), "e|--[3]---  |");
        assert_eq!(tab.lines[0].segments[0].chord, None);
        assert_eq!(tab.lines[1].lyrics(), "");
        assert_eq!(tab.lines[2].lyrics(), "# not a remark");
    }

    #[test]
    fn tab_characters_in_tablature_expand_to_eight_columns() {
        let result = parse("{sot}\ne|\t3|\nabc\tx\n{eot}");
        assert!(result.diagnostics.is_empty());
        let lines = &result.song.sections[0].lines;
        assert_eq!(lines[0].lyrics(), "e|      3|");
        assert_eq!(lines[1].lyrics(), "abc     x");
    }

    #[test]
    fn blank_lines_separate_stanzas_outside_environments() {
        let result = parse("{title: T}\n\n[C]one\n[G]two\n\n[C]three\n\n\n[D]four\n");
        assert!(result.diagnostics.is_empty());
        let sizes: Vec<_> = result.song.sections.iter().map(|s| s.lines.len()).collect();
        assert_eq!(sizes, [2, 1, 1]);
        assert!(
            result
                .song
                .sections
                .iter()
                .all(|s| s.kind == SectionKind::Unlabelled)
        );
    }

    #[test]
    fn blank_lines_inside_an_environment_keep_it_whole() {
        let result = parse("{sov: V}\n[C]one\n\n[G]two\n{eov}");
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.song.sections.len(), 1);
        assert_eq!(result.song.sections[0].lines.len(), 2);
    }

    #[test]
    fn lyrics_before_the_first_environment_are_not_an_error() {
        let result = parse("[C]Intro line\n{sov: Verse}\n[G]Verse line\n{eov}");
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        assert_eq!(
            labels(&result),
            [(SectionKind::Unlabelled, ""), (SectionKind::Verse, "Verse")]
        );
    }

    #[test]
    fn directive_arguments_follow_a_colon_or_whitespace() {
        for source in [
            "{title: Swing Low}",
            "{title Swing Low}",
            "{title:Swing Low}",
            "{title : Swing Low}",
            "{TITLE: Swing Low}",
            "{ t: Swing Low }",
        ] {
            let result = parse(source);
            assert!(result.diagnostics.is_empty(), "{source}");
            assert_eq!(
                result.song.first_meta("title"),
                Some("Swing Low"),
                "{source}"
            );
        }
    }

    #[test]
    fn environment_labels_come_from_attributes_or_the_argument() {
        let result = parse(
            "{start_of_verse: label=\"Verse 1\"}\n[C]a\n{end_of_verse}\n{start_of_verse label='Verse 2'}\n[C]b\n{end_of_verse}\n{sov Verse 3}\n[C]c\n{eov}\n{sob}\n[C]d\n{eob}\n{start_of_tab: label=\"Intro\"}\ne|--|\n{eot}",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        assert_eq!(
            labels(&result),
            [
                (SectionKind::Verse, "Verse 1"),
                (SectionKind::Verse, "Verse 2"),
                (SectionKind::Verse, "Verse 3"),
                (SectionKind::Bridge, ""),
                (SectionKind::Tab, "Intro"),
            ]
        );
    }

    #[test]
    fn comments_are_visible_lines_and_remarks_are_not() {
        let result = parse(
            "{sov}\n[C]a\n{c: Repeat x2}\n# editor note\n{eov}\n{comment_italic: Slowly}\n{cb: Boxed}\n{highlight: Loud}\n{comment:}",
        );
        let comments: Vec<_> = result
            .song
            .sections
            .iter()
            .flat_map(|s| &s.lines)
            .filter(|l| l.kind == LineKind::Comment)
            .map(Line::lyrics)
            .collect();
        assert_eq!(comments, ["Repeat x2", "Slowly", "Boxed", "Loud"]);
        assert_eq!(result.song.sections[0].lines.len(), 2);
        assert_eq!(messages(&result), ["empty comment"]);
    }

    #[test]
    fn chorus_recall_repeats_the_latest_chorus_by_reference() {
        let result = parse(
            "{soc: Chorus}\n[C]Sing\n{eoc}\n{sov}\n[G]Verse\n{eov}\n{chorus}\n{chorus: Last time}\n{chorus label=\"Out\"}",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        let sections = &result.song.sections;
        assert_eq!(sections.len(), 5);
        for (recall, label) in sections[2..].iter().zip(["Chorus", "Last time", "Out"]) {
            assert_eq!(recall.kind, SectionKind::Chorus);
            assert_eq!(recall.label, label);
            assert_eq!(recall.lines, sections[0].lines);
            assert_eq!(
                recall.recall_of,
                Some(0),
                "a recall of a recall points at the original"
            );
        }
        assert_eq!(sections[0].recall_of, None);
    }

    #[test]
    fn chorus_recall_before_any_chorus_is_diagnosed() {
        let result = parse("{sov}\n[C]a\n{eov}\n{chorus}");
        assert_eq!(result.song.sections.len(), 1);
        assert_eq!(messages(&result), ["chorus recall before any chorus"]);
    }

    #[test]
    fn mismatched_and_unclosed_environments_are_diagnosed() {
        let mismatched = parse("{sov}\n[C]a\n{eoc}");
        assert_eq!(
            messages(&mismatched),
            ["end_of_chorus closes a Verse section"]
        );
        let orphan = parse("[C]a\n{end_of_verse}");
        assert_eq!(messages(&orphan), ["end_of_verse without a start"]);
        let unclosed = parse("{soc}\n[C]a\n");
        assert_eq!(messages(&unclosed), ["Chorus section not closed"]);
        assert_eq!(unclosed.diagnostics[0].line, 2);
        assert_eq!(unclosed.song.sections[0].lines.len(), 1);
    }

    #[test]
    fn formatting_and_private_directives_are_accepted_and_ignored() {
        let result = parse(
            "{textfont: Times}\n{chordsize: 12}\n{choruscolour: red}\n{ts: 10}\n{titles: left}\n{x_mspro_capo: 2}\n{no_grid}\n[C]a",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        assert_eq!(result.song.sections.len(), 1);
    }

    #[test]
    fn layout_hints_are_reported_until_they_are_honoured() {
        let result = parse("{new_page}\n{colb}\n{columns: 2}");
        assert_eq!(result.diagnostics.len(), 3);
        assert!(
            messages(&result)
                .iter()
                .all(|m| m.starts_with("layout hint"))
        );
    }

    #[test]
    fn defines_are_stored_and_win_over_the_table() {
        let result = parse("{define: Am base-fret 5 frets x 0 2 2 1 0}\n[Am]Hi [C]there [Am]again");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.song.shape("Am").unwrap().base_fret, 5);
        assert_eq!(result.song.shape("C"), chords::lookup("C"));
        assert_eq!(result.song.chord_names(), ["Am", "C"]);
        assert_eq!(
            messages(&parse("{define: X}")),
            ["define needs a chord name and frets"]
        );
    }

    #[test]
    fn transpose_directive_shifts_the_chords_after_it() {
        let result = parse(
            "{key: G}\n[G]one [D]two\n{transpose: 2}\n[G]three [D/F#]four\n{transpose: 1f}\n[C]five\n{transpose}\n[C]six",
        );
        assert!(result.diagnostics.is_empty(), "{:?}", messages(&result));
        assert_eq!(
            result.song.chord_names(),
            ["G", "D", "A", "E/G#", "Db", "C"]
        );
        assert_eq!(
            result.song.first_meta("key"),
            Some("G"),
            "key stays as written"
        );
        assert_eq!(
            result.song.first_meta("key_actual"),
            Some("G"),
            "the last transpose cancelled"
        );
        let shifted = parse("{key: Am}\n{transpose: -2}\n[Am]x");
        assert_eq!(shifted.song.first_meta("key_actual"), Some("Gm"));
        assert_eq!(
            messages(&parse("{transpose: up}")),
            ["transpose needs semitones, not \"up\""]
        );
    }

    #[test]
    fn live_transpose_shifts_chords_and_keys_but_not_tablature() {
        let song =
            parse("{key: G}\n{capo: 2}\n[G]hi [D]there [*Riff]\n{sot}\ne|-[3]-|\n{eot}").song;
        let up = song.transposed(2);
        assert_eq!(up.transpose, 2);
        assert_eq!(up.chord_names(), ["A", "E"], "an annotation is no chord");
        assert_eq!(up.first_meta("key"), Some("A"));
        assert_eq!(up.first_meta("capo"), Some("2"));
        assert_eq!(up.sections[1].lines, song.sections[1].lines);
        assert_eq!(song.chord_names(), ["G", "D"], "the original is untouched");
        let back = up.transposed(-2);
        assert_eq!(back.transpose, 0);
        assert_eq!(back.chord_names(), song.chord_names());
        assert_eq!(song.transposed(0), song);
    }

    #[test]
    fn meta_is_the_generic_metadata_form() {
        let result =
            parse("{meta: mood reflective and slow}\n{meta: Artist Ada}\n{sortartist: Lovelace}");
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.song.first_meta("mood"), Some("reflective and slow"));
        assert_eq!(result.song.first_meta("artist"), Some("Ada"));
        assert_eq!(result.song.first_meta("sortartist"), Some("Lovelace"));
        assert_eq!(
            messages(&parse("{meta: lonely}")),
            ["meta needs a name and a value"]
        );
    }
}
