//! The library: every ChordPro file under the chosen folders, with the
//! metadata search shows. A scan runs on the host's worker, never on the
//! frame path, and reuses the previous scan's entry for any file whose size
//! and modification time have not changed, so a rescan reads only what moved.
//!
//! Each entry carries a content fingerprint, the SHA-256 of the file's
//! normalised text, which keys the sidecar store: a rename or a move to
//! another device keeps a song's tags and rating.

use crate::sha256;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// File extensions read as ChordPro songs, compared without case.
pub const SONG_EXTENSIONS: [&str; 6] = ["cho", "chopro", "chordpro", "crd", "pro", "cp"];

/// One song as search lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub fingerprint: String,
    /// The `{title}`, or the file name without its extension.
    pub title: String,
    pub artist: String,
    pub key: String,
    pub capo: String,
    pub tempo: String,
    pub year: String,
    /// Tags the file itself declares (`{tag: …}`, `{meta: tags …}`); shown,
    /// filtered on, and never written back.
    pub file_tags: Vec<String>,
    /// Every lyric line, lower-cased and joined, for an `l:` search.
    pub lyrics: String,
    size: u64,
    modified: Option<SystemTime>,
}

impl Entry {
    /// An entry for `text` read from `path`; pure, so tests need no files.
    pub fn from_source(path: &Path, text: &str) -> Self {
        let song = cpl_song::parse(text).song;
        let meta = |name: &str| song.first_meta(name).unwrap_or("").trim().to_owned();
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = Some(meta("title"))
            .filter(|t| !t.is_empty())
            .unwrap_or(stem);
        let mut file_tags: Vec<String> = Vec::new();
        for name in ["tag", "tags"] {
            for value in song.metadata.get(name).into_iter().flatten() {
                for tag in value.split([',', ' ']).map(normalise_tag) {
                    if !tag.is_empty() && !file_tags.contains(&tag) {
                        file_tags.push(tag);
                    }
                }
            }
        }
        let lyrics = song
            .sections
            .iter()
            .filter(|section| section.kind != cpl_song::SectionKind::Tab)
            .flat_map(|section| &section.lines)
            .map(|line| line.lyrics().to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            path: path.to_owned(),
            fingerprint: fingerprint(text),
            title,
            artist: meta("artist"),
            key: meta("key"),
            capo: meta("capo"),
            tempo: meta("tempo"),
            year: meta("year"),
            file_tags,
            lyrics,
            size: 0,
            modified: None,
        }
    }
}

/// A tag as stored and shown: trimmed, without a leading `#`, lower-cased.
pub fn normalise_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('#').trim().to_lowercase()
}

/// The text a fingerprint is taken over: no byte-order mark, `\n` line
/// endings, no trailing whitespace on a line or at the end. An editor that
/// changes only those does not orphan a song's tags.
pub fn normalise(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

/// The lowercase hex SHA-256 of the normalised text.
pub fn fingerprint(text: &str) -> String {
    sha256::hex(normalise(text).as_bytes())
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub folders: Vec<PathBuf>,
    /// Ordered by path.
    pub entries: Vec<Entry>,
}

impl Library {
    /// Every song under `folders`, recursively, ordered by path. Entries of
    /// `previous` are reused for files whose size and mtime are unchanged.
    /// Unreadable files and folders are skipped: a broken file must not hide
    /// the rest of the library.
    pub fn scan(folders: &[PathBuf], previous: Option<&Library>) -> Self {
        let known: HashMap<&Path, &Entry> = previous
            .map(|library| {
                library
                    .entries
                    .iter()
                    .map(|entry| (entry.path.as_path(), entry))
                    .collect()
            })
            .unwrap_or_default();
        let mut paths = Vec::new();
        for folder in folders {
            collect(folder, &mut paths, 0);
        }
        paths.sort();
        paths.dedup();
        let entries = paths
            .into_iter()
            .filter_map(|path| {
                let metadata = fs::metadata(&path).ok()?;
                let (size, modified) = (metadata.len(), metadata.modified().ok());
                if let Some(entry) = known.get(path.as_path())
                    && entry.size == size
                    && entry.modified == modified
                    && modified.is_some()
                {
                    return Some((*entry).clone());
                }
                let text = fs::read_to_string(&path).ok()?;
                let mut entry = Entry::from_source(&path, &text);
                entry.size = size;
                entry.modified = modified;
                Some(entry)
            })
            .collect();
        Self {
            folders: folders.to_vec(),
            entries,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Whether `path` names a ChordPro song by its extension.
pub fn is_song(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SONG_EXTENSIONS.iter().any(|k| k.eq_ignore_ascii_case(ext)))
}

/// Song files under `folder`, skipping hidden entries and stopping at a
/// sane depth so a symlink loop cannot hang a scan.
fn collect(folder: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(read) = fs::read_dir(folder) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect(&path, out, depth + 1),
            Ok(_) if is_song(&path) => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_and_trailing_space_keep_the_fingerprint() {
        let unix = "{title: A}\n[C]Hello\n";
        let windows = "\u{feff}{title: A}  \r\n[C]Hello\r\n\r\n";
        assert_eq!(fingerprint(unix), fingerprint(windows));
        assert_ne!(fingerprint(unix), fingerprint("{title: B}\n[C]Hello\n"));
        assert_eq!(fingerprint(unix).len(), 64);
    }

    #[test]
    fn an_entry_reads_metadata_tags_and_lyrics() {
        let entry = Entry::from_source(
            Path::new("songs/rising.cho"),
            "{title: House of the Rising Sun}\n{artist: Traditional}\n{key: Am}\n{capo: 0}\n{tempo: 76}\n{tag: Folk}\n{meta: tags #gig-set, slow}\n{sov}\nThere [Am]is a house\n{eov}\n{sot}\ne|--0--|\n{eot}",
        );
        assert_eq!(entry.title, "House of the Rising Sun");
        assert_eq!(entry.artist, "Traditional");
        assert_eq!((entry.key.as_str(), entry.capo.as_str()), ("Am", "0"));
        assert_eq!(entry.tempo, "76");
        assert_eq!(entry.file_tags, ["folk", "gig-set", "slow"]);
        assert_eq!(entry.lyrics, "there is a house");
    }

    #[test]
    fn a_song_without_a_title_is_named_by_its_file() {
        let entry = Entry::from_source(Path::new("x/Wayfaring Stranger.chopro"), "[Dm]Words");
        assert_eq!(entry.title, "Wayfaring Stranger");
        assert!(is_song(Path::new("a/B.CHO")));
        assert!(!is_song(Path::new("a/notes.txt")));
    }

    #[test]
    fn a_scan_finds_songs_recursively_and_reuses_unchanged_entries() {
        let root = std::env::temp_dir().join(format!("cpl-library-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("set/.hidden")).unwrap();
        fs::write(root.join("b.cho"), "{title: Bravo}").unwrap();
        fs::write(root.join("set/a.pro"), "{title: Alpha}").unwrap();
        fs::write(root.join("set/.hidden/c.cho"), "{title: Hidden}").unwrap();
        fs::write(root.join("set/notes.txt"), "not a song").unwrap();
        let first = Library::scan(std::slice::from_ref(&root), None);
        let titles: Vec<&str> = first.entries.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Bravo", "Alpha"]);
        // Unchanged files come back as the same entries; a changed one is reread.
        fs::write(root.join("b.cho"), "{title: Bravo, revised}").unwrap();
        let second = Library::scan(std::slice::from_ref(&root), Some(&first));
        assert_eq!(second.entries[0].title, "Bravo, revised");
        assert_eq!(second.entries[1], first.entries[1]);
        fs::remove_dir_all(&root).unwrap();
    }
}
