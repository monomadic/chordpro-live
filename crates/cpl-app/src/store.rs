//! The sidecar store: tags and ratings, kept in one `library.toml` beside the
//! config rather than in any song, because this app never rewrites a chart.
//! Each song is a `[song.<fingerprint>]` table:
//!
//! ```toml
//! [song.ba7816bf…]
//! title = "House of the Rising Sun"
//! rating = 4
//! tags = ["gig-set", "folk"]
//! opened = 2026-10-29T21:04:00Z
//! ```
//!
//! The title is a courtesy for a person reading the file; the fingerprint is
//! the key. Keys this build does not know are kept and written back, so a
//! newer build's data survives an older one. The parser reads exactly the
//! subset written here (strings, integers, arrays of strings), and a file it
//! cannot read is never overwritten.

use std::{collections::BTreeMap, fs, io, path::Path};

/// One song's sidecar record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record {
    pub title: String,
    /// 0 is unrated; 1–5 stars.
    pub rating: u8,
    pub tags: Vec<String>,
    /// When the song was last shown, in seconds since the Unix epoch; 0 is
    /// never. Written as a TOML UTC date-time.
    pub opened: u64,
    /// Lines for keys this build does not use, verbatim.
    other: Vec<String>,
}

impl Record {
    fn is_empty(&self) -> bool {
        self.rating == 0 && self.tags.is_empty() && self.opened == 0 && self.other.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Store {
    records: BTreeMap<String, Record>,
}

const HEADER: &str = "# chordpro-live library: tags and ratings, one table per song,\n\
# keyed by the SHA-256 of the song's normalised text. Song files are never\n\
# written; this file is the only place the app stores what you add.\n";

impl Store {
    pub fn record(&self, fingerprint: &str) -> Option<&Record> {
        self.records.get(fingerprint)
    }

    pub fn rating(&self, fingerprint: &str) -> u8 {
        self.record(fingerprint).map_or(0, |r| r.rating)
    }

    pub fn tags(&self, fingerprint: &str) -> &[String] {
        self.record(fingerprint).map_or(&[], |r| r.tags.as_slice())
    }

    /// When the song was last shown, in Unix seconds; 0 is never.
    pub fn opened(&self, fingerprint: &str) -> u64 {
        self.record(fingerprint).map_or(0, |r| r.opened)
    }

    /// Notes that the song was shown at `when` (Unix seconds).
    pub fn set_opened(&mut self, fingerprint: &str, title: &str, when: u64) {
        self.entry(fingerprint, title).opened = when;
        self.prune(fingerprint);
    }

    /// Sets a 0–5 rating; 0 clears it.
    pub fn set_rating(&mut self, fingerprint: &str, title: &str, rating: u8) {
        let record = self.entry(fingerprint, title);
        record.rating = rating.min(5);
        self.prune(fingerprint);
    }

    /// Adds `tag` if the song lacks it, removes it if it has it. Returns
    /// whether the song now has it.
    pub fn toggle_tag(&mut self, fingerprint: &str, title: &str, tag: &str) -> bool {
        let tag = crate::library::normalise_tag(tag);
        if tag.is_empty() {
            return false;
        }
        let record = self.entry(fingerprint, title);
        let added = if let Some(at) = record.tags.iter().position(|t| *t == tag) {
            record.tags.remove(at);
            false
        } else {
            record.tags.push(tag);
            true
        };
        self.prune(fingerprint);
        added
    }

    fn entry(&mut self, fingerprint: &str, title: &str) -> &mut Record {
        let record = self.records.entry(fingerprint.to_owned()).or_default();
        record.title = title.to_owned();
        record
    }

    fn prune(&mut self, fingerprint: &str) {
        if self.records.get(fingerprint).is_some_and(Record::is_empty) {
            self.records.remove(fingerprint);
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut records = BTreeMap::new();
        let mut current: Option<(String, Record)> = None;
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let at = |message: &str| format!("line {}: {message}", index + 1);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(header) = line.strip_prefix('[') {
                let key = header
                    .strip_suffix(']')
                    .and_then(|h| h.trim().strip_prefix("song."))
                    .filter(|key| !key.is_empty() && key.chars().all(|c| c.is_ascii_hexdigit()))
                    .ok_or_else(|| at("expected [song.<fingerprint>]"))?;
                if let Some((key, record)) = current.take() {
                    records.insert(key, record);
                }
                if records.contains_key(key) {
                    return Err(at("duplicate song"));
                }
                current = Some((key.to_ascii_lowercase(), Record::default()));
                continue;
            }
            let Some((_, record)) = current.as_mut() else {
                return Err(at("expected a [song.<fingerprint>] table first"));
            };
            let (key, value) = line
                .split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or_else(|| at("expected key = value"))?;
            match key {
                "title" => record.title = parse_string(value).ok_or_else(|| at("bad title"))?,
                "rating" => {
                    record.rating = value
                        .parse::<u8>()
                        .ok()
                        .filter(|r| *r <= 5)
                        .ok_or_else(|| at("rating must be 0 to 5"))?
                }
                "opened" => {
                    record.opened =
                        parse_datetime(value).ok_or_else(|| at("opened must be a UTC date-time"))?
                }
                "tags" => {
                    record.tags = parse_strings(value)
                        .ok_or_else(|| at("tags must be an array of strings"))?
                }
                _ => record.other.push(line.to_owned()),
            }
        }
        if let Some((key, record)) = current {
            records.insert(key, record);
        }
        Ok(Self { records })
    }

    pub fn to_toml(&self) -> String {
        let mut out = String::from(HEADER);
        for (key, record) in &self.records {
            out.push_str(&format!("\n[song.{key}]\n"));
            if !record.title.is_empty() {
                out.push_str(&format!("title = {}\n", quote(&record.title)));
            }
            if record.rating > 0 {
                out.push_str(&format!("rating = {}\n", record.rating));
            }
            if !record.tags.is_empty() {
                let tags: Vec<String> = record.tags.iter().map(|t| quote(t)).collect();
                out.push_str(&format!("tags = [{}]\n", tags.join(", ")));
            }
            if record.opened > 0 {
                out.push_str(&format!("opened = {}\n", format_datetime(record.opened)));
            }
            for line in &record.other {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// The store at `path`; a missing file is an empty store.
    pub fn load(path: &Path) -> Result<Self, String> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    /// Writes beside `path` and renames over it, so a crash never leaves a
    /// half-written store.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, self.to_toml())?;
        fs::rename(&temporary, path)
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `seconds` since the epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn format_datetime(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The `YYYY-MM-DDTHH:MM:SSZ` form `format_datetime` writes.
fn parse_datetime(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 || !value.is_ascii() || &value[4..5] != "-" || &value[7..8] != "-" {
        return None;
    }
    if !matches!(&value[10..11], "T" | "t" | " ") || &value[13..14] != ":" || &value[16..17] != ":"
    {
        return None;
    }
    if !matches!(&value[19..], "Z" | "z") {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let text = &value[range];
        text.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| text.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year as i64, month, day);
    let seconds = days * 86_400 + (hour * 3600 + minute * 60 + second) as i64;
    u64::try_from(seconds).ok()
}

fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A basic TOML string with the escapes `quote` writes.
fn parse_string(value: &str) -> Option<String> {
    let (text, rest) = string_prefix(value)?;
    rest.trim().is_empty().then_some(text)
}

/// The leading quoted string of `value` and what follows it.
fn string_prefix(value: &str) -> Option<(String, &str)> {
    let mut chars = value.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Some((out, &value[at + 2..])),
            '\\' => out.push(match chars.next()?.1 {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                't' => '\t',
                _ => return None,
            }),
            c => out.push(c),
        }
    }
    None
}

fn parse_strings(value: &str) -> Option<Vec<String>> {
    let mut rest = value.strip_prefix('[')?.trim_start();
    let mut out = Vec::new();
    loop {
        if let Some(after) = rest.strip_prefix(']') {
            return after.trim().is_empty().then_some(out);
        }
        let (text, after) = string_prefix(rest)?;
        out.push(text);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
        } else if !rest.starts_with(']') {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaa";
    const B: &str = "bbbb";

    #[test]
    fn ratings_and_tags_round_trip_through_the_file() {
        let mut store = Store::default();
        store.set_rating(A, "Rising \"Sun\"", 4);
        assert!(store.toggle_tag(A, "Rising \"Sun\"", "#Gig-Set"));
        store.toggle_tag(A, "Rising \"Sun\"", "folk");
        store.set_rating(B, "Other", 2);
        let text = store.to_toml();
        assert!(text.contains("[song.aaaa]\ntitle = \"Rising \\\"Sun\\\"\"\nrating = 4\ntags = [\"gig-set\", \"folk\"]\n"));
        assert_eq!(Store::parse(&text).unwrap(), store);
        assert_eq!(store.tags(A), ["gig-set", "folk"]);
        assert_eq!(store.rating(B), 2);
        assert_eq!(store.rating("cccc"), 0);
    }

    #[test]
    fn last_opened_round_trips_as_a_utc_date_time() {
        assert_eq!(format_datetime(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_datetime(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_datetime(1_761_771_845), "2025-10-29T21:04:05Z");
        for seconds in [0, 951_782_400, 1_761_771_845, 4_102_444_799] {
            assert_eq!(parse_datetime(&format_datetime(seconds)), Some(seconds));
        }
        assert_eq!(parse_datetime("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_datetime("2026-10-29"), None);
        let mut store = Store::default();
        store.set_opened(A, "A", 1_761_771_845);
        let text = store.to_toml();
        assert!(text.contains("opened = 2025-10-29T21:04:05Z\n"));
        assert_eq!(Store::parse(&text).unwrap().opened(A), 1_761_771_845);
        assert_eq!(store.opened(B), 0);
        store.set_opened(A, "A", 0);
        assert!(store.record(A).is_none());
    }

    #[test]
    fn toggling_twice_removes_and_an_empty_record_disappears() {
        let mut store = Store::default();
        assert!(store.toggle_tag(A, "A", "folk"));
        assert!(!store.toggle_tag(A, "A", "folk"));
        assert!(store.record(A).is_none());
        store.set_rating(A, "A", 3);
        store.set_rating(A, "A", 0);
        assert!(store.record(A).is_none());
    }

    #[test]
    fn unknown_keys_survive_a_rewrite() {
        let text = "[song.abc]\nrating = 5\nlast_opened = 2026-10-29\ntimings = [1, 2]\n";
        let mut store = Store::parse(text).unwrap();
        store.toggle_tag("abc", "", "new");
        let written = store.to_toml();
        assert!(written.contains("last_opened = 2026-10-29\ntimings = [1, 2]\n"));
        assert_eq!(Store::parse(&written).unwrap().rating("abc"), 5);
    }

    #[test]
    fn a_file_it_cannot_read_is_an_error_not_an_empty_store() {
        for bad in [
            "rating = 3",
            "[song.xyz]\nrating = 3",
            "[song.abc]\nrating = 9",
            "[song.abc]\ntags = [\"a\" \"b\"]",
            "[song.abc]\n[song.abc]",
        ] {
            assert!(Store::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn saving_writes_atomically_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("cpl-store-{}", std::process::id()));
        let path = dir.join("nested/library.toml");
        assert_eq!(Store::load(&path).unwrap(), Store::default());
        let mut store = Store::default();
        store.set_rating(A, "A", 5);
        store.save(&path).unwrap();
        assert_eq!(Store::load(&path).unwrap(), store);
        assert!(!path.with_extension("toml.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
