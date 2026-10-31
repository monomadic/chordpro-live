//! `cpl conform`: parses the same ChordPro with the reference implementation
//! and with `cpl-song`, and compares what each understood: the songs in a
//! file, their metadata, their section structure, the chords in order and
//! the lyric, comment and tab text. The reference speaks through a small
//! output backend in `scripts/conform/` that writes its parse as JSON.
//!
//! Two tables come out of it, both generated: one row per spec directive
//! (each with a probe song written below) and one row per corpus file. The
//! song note's status table is the first, written between markers, so it
//! cannot drift from the code.

use crate::json::{self, Json};
use cpl_song::{LineKind, SectionKind, Song};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

/// The metadata both parsers are held to: the spec's standard items.
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

/// One status-table row: what the spec names it, and a song that uses it.
/// A probe is the smallest song that shows whether the construct is
/// understood the way the reference understands it.
const PROBES: &[(&str, &str)] = &[
    ("{title}", "{title: Probe}\n[C]Sing"),
    ("{subtitle}", "{title: T}\n{subtitle: Under}\n[C]Sing"),
    ("{artist}", "{title: T}\n{artist: Ada}\n[C]Sing"),
    (
        "{sortartist}",
        "{title: T}\n{sortartist: Lovelace}\n[C]Sing",
    ),
    ("{composer}", "{title: T}\n{composer: Bach}\n[C]Sing"),
    ("{lyricist}", "{title: T}\n{lyricist: Wordsworth}\n[C]Sing"),
    ("{arranger}", "{title: T}\n{arranger: Someone}\n[C]Sing"),
    (
        "{copyright}",
        "{title: T}\n{copyright: 2026 Nobody}\n[C]Sing",
    ),
    ("{album}", "{title: T}\n{album: Songs}\n[C]Sing"),
    ("{year}", "{title: T}\n{year: 1964}\n[C]Sing"),
    ("{key}", "{title: T}\n{key: Am}\n[Am]Sing"),
    ("{time}", "{title: T}\n{time: 6/8}\n[C]Sing"),
    ("{tempo}", "{title: T}\n{tempo: 96}\n[C]Sing"),
    ("{duration}", "{title: T}\n{duration: 3:30}\n[C]Sing"),
    ("{capo}", "{title: T}\n{capo: 2}\n[C]Sing"),
    (
        "{sorttitle}",
        "{title: The T}\n{sorttitle: T, The}\n[C]Sing",
    ),
    ("{tag}", "{title: T}\n{tag: folk}\n{tag: live}\n[C]Sing"),
    (
        "{meta}",
        "{title: T}\n{meta: composer Bach}\n{meta: artist Ada}\n[C]Sing",
    ),
    ("{comment}", "{title: T}\n{comment: Slowly}\n[C]Sing"),
    ("{comment_italic}", "{title: T}\n{ci: Softly}\n[C]Sing"),
    ("{comment_box}", "{title: T}\n{cb: Boxed}\n[C]Sing"),
    ("{highlight}", "{title: T}\n{highlight: Loud}\n[C]Sing"),
    (
        "{start_of_verse}",
        "{title: T}\n{start_of_verse: Verse 1}\n[C]One [G]two\n{end_of_verse}\n{sov label=\"Verse 2\"}\n[C]Three\n{eov}",
    ),
    (
        "{start_of_chorus}",
        "{title: T}\n{start_of_chorus}\n[G]Sing it [D/F#]out\n{end_of_chorus}",
    ),
    (
        "{start_of_bridge}",
        "{title: T}\n{start_of_bridge: Bridge}\n[Em]Over\n{end_of_bridge}",
    ),
    (
        "{start_of_tab}",
        "{title: T}\n{start_of_tab}\ne|--[3]--0--|\nB|--1-----1--|\n{end_of_tab}",
    ),
    (
        "{start_of_grid}",
        "{title: T}\n{start_of_grid}\n| C . . . | G . . . |\n{end_of_grid}",
    ),
    (
        "{chorus}",
        "{title: T}\n{soc}\n[G]Sing\n{eoc}\n{sov}\n[C]Verse\n{eov}\n{chorus}",
    ),
    (
        "{define}",
        "{title: T}\n{define: Am base-fret 1 frets x 0 2 2 1 0}\n[Am]Sing",
    ),
    ("{chord}", "{title: T}\n{chord: Am}\n[Am]Sing"),
    (
        "{transpose}",
        "{title: T}\n{key: G}\n[G]One\n{transpose: 2}\n[G]Two [Bb]three\n{transpose}\n[G]Four",
    ),
    (
        "{start_of_abc}",
        "{title: T}\n{start_of_abc}\nX:1\nK:C\nCDEF|\n{end_of_abc}\n[C]Sing",
    ),
    (
        "{start_of_ly}",
        "{title: T}\n{start_of_ly}\n\\relative c' { c d e f }\n{end_of_ly}\n[C]Sing",
    ),
    (
        "{start_of_svg}",
        "{title: T}\n{start_of_svg}\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n{end_of_svg}\n[C]Sing",
    ),
    (
        "{start_of_textblock}",
        "{title: T}\n{start_of_textblock}\nSome text\n{end_of_textblock}\n[C]Sing",
    ),
    (
        "{new_song}",
        "{title: One}\n[C]First\n{new_song}\n{title: Two}\n[G]Second",
    ),
    ("{new_page}", "{title: T}\n[C]One\n{new_page}\n[G]Two"),
    ("{new_physical_page}", "{title: T}\n[C]One\n{npp}\n[G]Two"),
    (
        "{column_break}",
        "{title: T}\n[C]One\n{column_break}\n[G]Two",
    ),
    ("{columns}", "{title: T}\n{columns: 2}\n[C]One"),
    ("{pagetype}", "{title: T}\n{pagetype: a4}\n[C]One"),
    ("{titles}", "{title: T}\n{titles: left}\n[C]One"),
    ("{diagrams}", "{title: T}\n{diagrams: off}\n[C]One"),
    ("{grid}", "{title: T}\n{grid}\n[C]One"),
    (
        "{textfont} and the font, size, colour families",
        "{title: T}\n{textfont: Times}\n{chordsize: 12}\n{choruscolour: red}\n[C]One",
    ),
    ("{image}", "{title: T}\n{image: src=\"cover.png\"}\n[C]One"),
    (
        "{x_…} private directives",
        "{title: T}\n{x_mine: kept}\n[C]One",
    ),
    ("[*annotation]", "{title: T}\n[C]Sing [*Riff x2]along"),
    ("chord-only line", "{title: T}\n[C] [G] [Am] [F]"),
    ("mid-word chord", "{title: T}\n[C]Ama[G]zing [D]grace"),
    ("<b> markup", "{title: T}\n[C]Sing <b>loud</b> now"),
    (
        "%{…} substitution",
        "{title: Title}\n{comment: %{title}}\n[C]Sing",
    ),
    (
        "{comment-guitar} selector",
        "{title: T}\n{comment-guitar: Capo up}\n[C]Sing",
    ),
];

/// What a song is, for comparison: the same shape built from either parse.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub meta: BTreeMap<String, Vec<String>>,
    pub blocks: Vec<Block>,
}

/// A run of lines in one environment: a verse, a chorus, a recalled chorus,
/// a tab, or a stanza no environment encloses (kind "").
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    pub kind: String,
    pub label: String,
    pub lines: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// (chord, lyric after it) pairs; the chord is "" before the first.
    Lyrics(Vec<(String, String)>),
    Comment(String),
    Tab(String),
    /// Something the reference produced that this comparison does not model
    /// (a grid line, an embedded delegate), by its type.
    Other(String),
}

impl Item {
    fn text(&self) -> String {
        match self {
            Item::Lyrics(pairs) => pairs.iter().map(|(_, lyric)| lyric.as_str()).collect(),
            Item::Comment(text) => format!("comment: {text}"),
            Item::Tab(text) => format!("tab: {text}"),
            Item::Other(kind) => format!("[{kind}]"),
        }
    }
}

/// The comparison model of what `cpl-song` parsed. A source holds one song
/// as this parser reads it.
pub fn from_song(song: &Song) -> Model {
    let meta = METADATA
        .iter()
        .filter_map(|&name| Some((name.to_owned(), song.metadata.get(name)?.clone())))
        .collect();
    let blocks = song
        .sections
        .iter()
        .map(|section| {
            let kind = match (section.recall_of, section.kind) {
                (Some(_), _) => "chorus recall",
                (None, SectionKind::Verse) => "verse",
                (None, SectionKind::Chorus) => "chorus",
                (None, SectionKind::Bridge) => "bridge",
                (None, SectionKind::Tab) => "tab",
                (None, SectionKind::Unlabelled) => "",
            };
            // A recall carries its own label, the chorus's when it gives none.
            let label = section.label.clone();
            let lines = section
                .lines
                .iter()
                .map(|line| match (section.kind, line.kind) {
                    (_, LineKind::Comment) => Item::Comment(line.lyrics()),
                    (SectionKind::Tab, _) => Item::Tab(line.lyrics()),
                    _ => Item::Lyrics(
                        line.segments
                            .iter()
                            .map(|s| (s.chord.clone().unwrap_or_default(), s.lyric.clone()))
                            .collect(),
                    ),
                })
                .collect();
            Block {
                kind: kind.into(),
                label,
                lines,
            }
        })
        .collect();
    Model { meta, blocks }
}

/// The comparison model of one song from the reference's JSON.
pub fn from_reference(song: &Json) -> Model {
    let meta = METADATA
        .iter()
        .filter_map(|&name| {
            let values = song.get("meta")?.get(name)?.strings();
            (!values.is_empty()).then(|| (name.to_owned(), values))
        })
        .collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut open = false;
    let place = |blocks: &mut Vec<Block>, open: &mut bool, context: &str| {
        if !*open || blocks.last().is_none_or(|b| b.kind != context) {
            blocks.push(Block {
                kind: context.into(),
                ..Block::default()
            });
            *open = true;
        }
    };
    for element in song.get("body").map_or(&[][..], Json::as_array) {
        let field = |name: &str| element.get(name).and_then(Json::as_str).unwrap_or("");
        let context = field("context");
        match field("type") {
            "set" if field("name") == "context" => open = false,
            "set" if field("name") == "label" => {
                place(&mut blocks, &mut open, context);
                blocks.last_mut().unwrap().label = field("value").into();
            }
            "empty" if context.is_empty() => open = false,
            "rechorus" => {
                let chorus = element.get("chorus").map_or(&[][..], Json::as_array);
                let inner = from_reference(&Json::Object(BTreeMap::from([(
                    "body".to_owned(),
                    Json::Array(chorus.to_vec()),
                )])));
                let mut block = inner.blocks.into_iter().next().unwrap_or_default();
                block.kind = "chorus recall".into();
                blocks.push(block);
                open = false;
            }
            "songline" => {
                let chords = element.get("chords").map_or(Vec::new(), Json::strings);
                let phrases = element.get("phrases").map_or(Vec::new(), Json::strings);
                let pairs = phrases
                    .into_iter()
                    .enumerate()
                    .map(|(i, phrase)| (chords.get(i).cloned().unwrap_or_default(), phrase))
                    .collect();
                place(&mut blocks, &mut open, context);
                blocks.last_mut().unwrap().lines.push(Item::Lyrics(pairs));
            }
            kind @ ("comment" | "comment_italic" | "comment_box" | "highlight") => {
                let _ = kind;
                place(&mut blocks, &mut open, context);
                let text = field("text").to_owned();
                blocks.last_mut().unwrap().lines.push(Item::Comment(text));
            }
            "tabline" => {
                place(&mut blocks, &mut open, context);
                let text = field("text").to_owned();
                blocks.last_mut().unwrap().lines.push(Item::Tab(text));
            }
            kind @ ("gridline" | "delegate" | "textblock" | "image") => {
                place(&mut blocks, &mut open, context);
                blocks
                    .last_mut()
                    .unwrap()
                    .lines
                    .push(Item::Other(kind.into()));
            }
            // Layout, diagrams, settings and ignored lines change nothing
            // this comparison models.
            _ => {}
        }
    }
    Model { meta, blocks }
}

/// Where two parses of one source first part.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    /// songs, metadata, structure, chords or lyrics.
    pub area: &'static str,
    pub detail: String,
}

fn first_difference<T: PartialEq + std::fmt::Debug>(
    area: &'static str,
    ours: &[T],
    theirs: &[T],
    show: impl Fn(&T) -> String,
) -> Option<Difference> {
    let at = ours
        .iter()
        .zip(theirs)
        .position(|(a, b)| a != b)
        .or_else(|| (ours.len() != theirs.len()).then(|| ours.len().min(theirs.len())))?;
    let side = |list: &[T]| list.get(at).map_or("nothing".to_owned(), &show);
    Some(Difference {
        area,
        detail: format!(
            "item {}: cpl-song has {}, the reference has {}",
            at + 1,
            side(ours),
            side(theirs)
        ),
    })
}

/// Every area in which the parses differ, each with its first difference.
pub fn compare(ours: &[Model], theirs: &[Model]) -> Vec<Difference> {
    if ours.len() != theirs.len() {
        return vec![Difference {
            area: "songs",
            detail: format!(
                "cpl-song reads {} song(s), the reference {}",
                ours.len(),
                theirs.len()
            ),
        }];
    }
    let mut found = Vec::new();
    for (ours, theirs) in ours.iter().zip(theirs) {
        let meta = |m: &Model| -> Vec<(String, Vec<String>)> {
            m.meta.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let structure = |m: &Model| -> Vec<(String, String, usize)> {
            m.blocks
                .iter()
                .map(|b| (b.kind.clone(), b.label.clone(), b.lines.len()))
                .collect()
        };
        let chords = |m: &Model| -> Vec<String> {
            m.blocks
                .iter()
                .flat_map(|b| &b.lines)
                .flat_map(|item| match item {
                    Item::Lyrics(pairs) => pairs
                        .iter()
                        .map(|(chord, _)| chord.clone())
                        .filter(|chord| !chord.is_empty())
                        .collect(),
                    _ => Vec::new(),
                })
                .collect()
        };
        let lyrics = |m: &Model| -> Vec<String> {
            m.blocks
                .iter()
                .flat_map(|b| &b.lines)
                .map(Item::text)
                .collect()
        };
        found.extend(first_difference(
            "metadata",
            &meta(ours),
            &meta(theirs),
            |(k, v)| format!("{k} = {v:?}"),
        ));
        found.extend(first_difference(
            "structure",
            &structure(ours),
            &structure(theirs),
            |(kind, label, lines)| {
                let kind = if kind.is_empty() { "stanza" } else { kind };
                format!("{kind} {label:?} of {lines} line(s)")
            },
        ));
        found.extend(first_difference(
            "chords",
            &chords(ours),
            &chords(theirs),
            |c| format!("{c:?}"),
        ));
        found.extend(first_difference(
            "lyrics",
            &lyrics(ours),
            &lyrics(theirs),
            |l| format!("{l:?}"),
        ));
    }
    found
}

/// The installed reference: `$CPL_CHORDPRO`, else `chordpro` on the PATH,
/// else the per-user CPAN install in `~/perl5`.
struct Reference {
    program: PathBuf,
    perl5lib: String,
}

impl Reference {
    fn find() -> Option<Self> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let local = home.as_ref().map(|h| h.join("perl5"));
        let program = env::var_os("CPL_CHORDPRO")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(|| {
                env::split_paths(&env::var_os("PATH")?)
                    .map(|dir| dir.join("chordpro"))
                    .find(|path| path.is_file())
            })
            .or_else(|| {
                local
                    .as_ref()
                    .map(|l| l.join("bin/chordpro"))
                    .filter(|p| p.is_file())
            })?;
        let backend = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/conform");
        let mut paths = vec![backend];
        if let Some(local) = local {
            paths.push(local.join("lib/perl5"));
        }
        if let Some(existing) = env::var_os("PERL5LIB") {
            paths.extend(env::split_paths(&existing));
        }
        let perl5lib = env::join_paths(paths).ok()?.to_string_lossy().into_owned();
        Some(Self { program, perl5lib })
    }

    /// The reference's own example songs, beside its installed modules.
    fn examples(&self) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = env::split_paths(&self.perl5lib)
            .map(|dir| dir.join("ChordPro/res/examples"))
            .filter_map(|dir| fs::read_dir(dir).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "cho"))
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// The reference's parse of `path`, one model per song.
    fn parse(&self, path: &Path, scratch: &Path) -> Result<Vec<Model>, String> {
        let out = scratch.join("reference.json");
        let _ = fs::remove_file(&out);
        let run = Command::new(&self.program)
            .env("PERL5LIB", &self.perl5lib)
            .args(["--no-default-configs", "--generate=CplJSON", "-o"])
            .arg(&out)
            .arg(path)
            .output()
            .map_err(|e| format!("{}: {e}", self.program.display()))?;
        let text = fs::read_to_string(&out).map_err(|_| {
            format!(
                "the reference wrote nothing: {}",
                String::from_utf8_lossy(&run.stderr).trim()
            )
        })?;
        let value = json::parse(&text)?;
        Ok(value
            .get("songs")
            .map_or(&[][..], Json::as_array)
            .iter()
            .map(from_reference)
            .collect())
    }
}

/// How `cpl-song` takes a probe, from its own diagnostics.
fn handling(name: &str, source: &str) -> String {
    let result = cpl_song::parse(source);
    if let Some(first) = result.diagnostics.first() {
        return format!("diagnosed: {}", first.message);
    }
    let formatting = name.starts_with('{')
        && name[1..]
            .split(['}', ' '])
            .next()
            .is_some_and(cpl_song::is_formatting);
    if formatting || name.starts_with("{x_") {
        "accepted, ignored".into()
    } else {
        "no diagnostic".into()
    }
}

fn verdict(differences: &[Difference]) -> String {
    match differences.first() {
        None => "yes".into(),
        Some(first) => {
            let areas: Vec<&str> = differences.iter().map(|d| d.area).collect();
            format!("no ({}): {}", areas.join(", "), first.detail)
        }
    }
}

/// Escapes a table cell: pipes, and newlines a probe might carry.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

const BEGIN: &str = "<!-- conform:begin (generated by `cpl conform --table`; do not edit) -->";
const END: &str = "<!-- conform:end -->";

pub fn main(mut args: Vec<String>) -> ExitCode {
    let mut table = None;
    let mut check = false;
    let mut files = Vec::new();
    while !args.is_empty() {
        let arg = args.remove(0);
        match arg.as_str() {
            "--table" if !args.is_empty() => table = Some(PathBuf::from(args.remove(0))),
            "--check" => check = true,
            _ if !arg.starts_with('-') => files.push(PathBuf::from(arg)),
            _ => {
                eprintln!("cpl conform: bad argument {arg:?}\n{}", crate::USAGE);
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(reference) = Reference::find() else {
        eprintln!(
            "cpl conform needs the reference ChordPro: `cpan App::Music::ChordPro`, \
             or set CPL_CHORDPRO to its `chordpro` program"
        );
        return ExitCode::FAILURE;
    };
    let version = Command::new(&reference.program)
        .env("PERL5LIB", &reference.perl5lib)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let scratch = env::temp_dir().join(format!("cpl-conform-{}", std::process::id()));
    if let Err(error) = fs::create_dir_all(&scratch) {
        eprintln!("{}: {error}", scratch.display());
        return ExitCode::FAILURE;
    }

    // The directive probes.
    let mut probe_rows = Vec::new();
    let mut probe_failures = 0;
    for (index, (name, source)) in PROBES.iter().enumerate() {
        let path = scratch.join(format!("probe-{index}.cho"));
        let theirs = fs::write(&path, source)
            .map_err(|e| e.to_string())
            .and_then(|()| reference.parse(&path, &scratch));
        let ours = vec![from_song(&cpl_song::parse(source).song)];
        let agrees = match &theirs {
            Ok(theirs) => verdict(&compare(&ours, theirs)),
            Err(error) => format!("reference failed: {error}"),
        };
        if agrees != "yes" {
            probe_failures += 1;
        }
        probe_rows.push((name.to_string(), handling(name, source), agrees));
    }

    // The corpus: the files named, else testdata and the reference's examples.
    if files.is_empty() {
        files.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata"));
        files.extend(reference.examples());
    }
    let mut corpus = Vec::new();
    for arg in &files {
        let paths = if arg.is_dir() {
            let mut found: Vec<PathBuf> = fs::read_dir(arg)
                .map(|dir| {
                    dir.filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| {
                            p.extension().is_some_and(|e| {
                                matches!(e.to_str(), Some("cho" | "chopro" | "chordpro" | "crd"))
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            found.sort();
            found
        } else {
            vec![arg.clone()]
        };
        corpus.extend(paths);
    }
    let mut file_rows = Vec::new();
    let mut file_failures = 0;
    for path in &corpus {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let agrees = match fs::read_to_string(path) {
            Err(error) => format!("unreadable: {error}"),
            Ok(source) => {
                let ours = vec![from_song(&cpl_song::parse(&source).song)];
                match reference.parse(path, &scratch) {
                    Ok(theirs) => verdict(&compare(&ours, &theirs)),
                    Err(error) => format!("reference failed: {error}"),
                }
            }
        };
        if agrees != "yes" {
            file_failures += 1;
        }
        file_rows.push((name, agrees));
    }
    let _ = fs::remove_dir_all(&scratch);

    let mut text = format!(
        "Compared with the reference implementation ({version}): {} of {} constructs and \
         {} of {} files parse alike.\n\n\
         | Construct | cpl-song says | Same parse as the reference |\n|---|---|---|\n",
        PROBES.len() - probe_failures,
        PROBES.len(),
        corpus.len() - file_failures,
        corpus.len()
    );
    for (name, ours, agrees) in &probe_rows {
        text.push_str(&format!(
            "| `{}` | {} | {} |\n",
            cell(name),
            cell(ours),
            cell(agrees)
        ));
    }
    text.push_str("\n| File | Same parse as the reference |\n|---|---|\n");
    for (name, agrees) in &file_rows {
        text.push_str(&format!("| {} | {} |\n", cell(name), cell(agrees)));
    }
    print!("{text}");

    if let Some(path) = table {
        let Ok(note) = fs::read_to_string(&path) else {
            eprintln!("{}: cannot read", path.display());
            return ExitCode::FAILURE;
        };
        let (Some(start), Some(end)) = (note.find(BEGIN), note.find(END)) else {
            eprintln!("{}: no {BEGIN} … {END} markers", path.display());
            return ExitCode::FAILURE;
        };
        let updated = format!("{}{BEGIN}\n{text}{}", &note[..start], &note[end..]);
        if check {
            if updated != note {
                eprintln!(
                    "{}: the conformance table is out of date; run `cpl conform --table {}`",
                    path.display(),
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        } else if updated != note
            && let Err(error) = fs::write(&path, updated)
        {
            eprintln!("{}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference's JSON for the probe song in `tests/fixtures/conform`,
    /// captured from chordpro 6.101.0 so these tests need no Perl.
    fn reference_fixture() -> Vec<Model> {
        let text = include_str!("../tests/fixtures/conform/probe.json");
        json::parse(text)
            .unwrap()
            .get("songs")
            .unwrap()
            .as_array()
            .iter()
            .map(from_reference)
            .collect()
    }

    #[test]
    fn the_reference_body_becomes_blocks() {
        let theirs = reference_fixture();
        assert_eq!(theirs.len(), 1);
        let kinds: Vec<(&str, &str)> = theirs[0]
            .blocks
            .iter()
            .map(|b| (b.kind.as_str(), b.label.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("", ""),
                ("chorus", "Chorus"),
                ("", ""),
                ("verse", ""),
                ("chorus recall", "Chorus"),
                ("chorus recall", "Last"),
                ("", ""),
                ("tab", "Intro"),
                ("bridge", ""),
            ]
        );
        assert_eq!(theirs[0].meta["key"], ["G"]);
        assert!(
            !theirs[0].meta.contains_key("_key"),
            "internal items are not compared"
        );
        assert_eq!(
            theirs[0].blocks[1].lines,
            [Item::Lyrics(vec![
                ("G".into(), "Sing it ".into()),
                ("D/F#".into(), "out".into())
            ])]
        );
    }

    #[test]
    fn the_probe_song_differs_only_where_cpl_song_is_known_to() {
        let source = include_str!("../tests/fixtures/conform/probe.cho");
        let ours = vec![from_song(&cpl_song::parse(source).song)];
        let differences = compare(&ours, &reference_fixture());
        let areas: Vec<&str> = differences.iter().map(|d| d.area).collect();
        // `[*Riff x2]` is an annotation the reference shows without its
        // star; cpl-song keeps it as a chord name, and the tab is labelled
        // the same in both.
        assert_eq!(areas, ["chords"], "{differences:#?}");
        assert!(
            differences[0].detail.contains("\"*Riff x2\""),
            "{}",
            differences[0].detail
        );
    }

    #[test]
    fn a_song_count_difference_is_reported_alone() {
        let one = vec![Model::default()];
        let two = vec![Model::default(), Model::default()];
        let differences = compare(&one, &two);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].area, "songs");
        assert!(compare(&one, &one).is_empty());
    }

    #[test]
    fn handling_reads_cpl_songs_own_diagnostics() {
        assert_eq!(handling("{title}", "{title: T}"), "no diagnostic");
        assert_eq!(
            handling("{textfont}", "{textfont: Times}"),
            "accepted, ignored"
        );
        assert_eq!(handling("{x_mine}", "{x_mine: 1}"), "accepted, ignored");
        assert!(handling("{new_song}", "{new_song}").starts_with("diagnosed: unsupported"));
    }
}
