//! Shifting chord names by semitones. Ported from
//! chordpro-tui/internal/chordpro/transpose.go with its tests; ChordPro 6's
//! `{transpose: 2f}` and `2s` add forced flat and sharp spellings.

/// How a shifted root is spelled when it lands on a black key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Spelling {
    /// The lead-sheet spelling chordpro-tui settled on, whatever the key:
    /// E♭ and B♭ are flats; C♯, F♯ and G♯ are sharps.
    #[default]
    Fixed,
    Sharps,
    Flats,
}

const FIXED: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B",
];
const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

impl Spelling {
    fn name(self, pitch_class: usize) -> &'static str {
        match self {
            Spelling::Fixed => FIXED[pitch_class],
            Spelling::Sharps => SHARPS[pitch_class],
            Spelling::Flats => FLATS[pitch_class],
        }
    }
}

/// A root note at the start of `name`: its pitch class (0 = C) and its
/// length in bytes. `♯` and `♭` count as accidentals as `#` and `b` do.
fn root(name: &str) -> Option<(i32, usize)> {
    let base = match name.chars().next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = &name[1..];
    Some(match rest.chars().next() {
        Some('#') => (base + 1, 2),
        Some('b') => (base - 1, 2),
        Some(c @ ('♯' | '♭')) => (base + if c == '♯' { 1 } else { -1 }, 1 + c.len_utf8()),
        _ => (base, 1),
    })
}

/// `chord` shifted by `semitones`, its bass note too. A name that does not
/// start with a note (`N.C.`, `%`, an annotation) comes back unchanged, and
/// so does every chord when `semitones` is zero, so an untransposed song
/// keeps the spelling its author wrote.
pub fn chord(chord: &str, semitones: i32, spelling: Spelling) -> String {
    if semitones == 0 {
        return chord.to_owned();
    }
    if let Some((upper, bass)) = chord.split_once('/') {
        return format!(
            "{}/{}",
            self::chord(upper, semitones, spelling),
            self::chord(bass, semitones, spelling)
        );
    }
    match root(chord) {
        Some((pitch, len)) => {
            let shifted = (pitch + semitones).rem_euclid(12) as usize;
            format!("{}{}", spelling.name(shifted), &chord[len..])
        }
        None => chord.to_owned(),
    }
}

/// A `{transpose}` argument: `2`, `-3`, `+1`, and `2f` or `2s` to force flat
/// or sharp spellings. Empty cancels, which is a shift of zero.
pub fn parse_directive(value: &str) -> Option<(i32, Spelling)> {
    let value = value.trim();
    if value.is_empty() {
        return Some((0, Spelling::Fixed));
    }
    let (number, spelling) = match value.chars().last()? {
        'f' | 'F' => (&value[..value.len() - 1], Spelling::Flats),
        's' | 'S' => (&value[..value.len() - 1], Spelling::Sharps),
        _ => (value, Spelling::Fixed),
    };
    let number = number.trim();
    let number = number.strip_prefix('+').unwrap_or(number);
    Some((number.parse().ok()?, spelling))
}

/// A signed offset for the status strip and header: `+2`, `−3` (a true
/// minus, which lines up with the plus).
pub fn signed(semitones: i32) -> String {
    match semitones {
        n if n > 0 => format!("+{n}"),
        n if n < 0 => format!("−{}", -n),
        _ => "0".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, n: i32) -> String {
        chord(name, n, Spelling::Fixed)
    }

    #[test]
    fn chords_shift_by_semitones() {
        for (name, n, want) in [
            ("C", 2, "D"),
            ("G", 2, "A"),
            ("A", 3, "C"),
            ("C", 1, "C#"),
            ("E", 1, "F"),
            ("B", 1, "C"),
            ("Am", 2, "Bm"),
            ("G7", 2, "A7"),
            ("Cmaj7", 5, "Fmaj7"),
            ("F#m", 1, "Gm"),
            ("C", 12, "C"),
            ("C", -1, "B"),
            ("D", -2, "C"),
        ] {
            assert_eq!(t(name, n), want, "{name} {n:+}");
        }
    }

    #[test]
    fn the_fixed_spelling_is_independent_of_the_key() {
        for (name, want) in [
            ("C", "C#"),
            ("D", "Eb"),
            ("F", "F#"),
            ("G", "G#"),
            ("A", "Bb"),
            ("Db", "D"),
            ("A#", "B"),
            ("Gb", "G"),
            ("B♭", "B"),
            ("F♯m", "Gm"),
        ] {
            assert_eq!(t(name, 1), want, "{name}");
        }
    }

    #[test]
    fn no_shift_keeps_the_written_spelling() {
        for name in ["A#", "Db", "Gb", "D#m7", "Ab/C"] {
            assert_eq!(t(name, 0), name);
        }
    }

    #[test]
    fn slash_chords_shift_both_notes() {
        assert_eq!(t("G/B", 2), "A/C#");
        assert_eq!(t("D/F#", 1), "Eb/G");
        assert_eq!(t("E/G#", 12), "E/G#");
    }

    #[test]
    fn non_chords_are_unchanged() {
        for name in ["", "N.C.", "%", "x4", "*Riff x2"] {
            assert_eq!(t(name, 3), name);
        }
    }

    #[test]
    fn forced_spellings() {
        assert_eq!(chord("C", 1, Spelling::Flats), "Db");
        assert_eq!(chord("C", 3, Spelling::Sharps), "D#");
        assert_eq!(chord("F/A", 1, Spelling::Flats), "Gb/Bb");
        assert_eq!(chord("C", 10, Spelling::Sharps), "A#");
    }

    #[test]
    fn directive_arguments() {
        assert_eq!(parse_directive("2"), Some((2, Spelling::Fixed)));
        assert_eq!(parse_directive("+2"), Some((2, Spelling::Fixed)));
        assert_eq!(parse_directive("-3"), Some((-3, Spelling::Fixed)));
        assert_eq!(parse_directive("2f"), Some((2, Spelling::Flats)));
        assert_eq!(parse_directive("-1s"), Some((-1, Spelling::Sharps)));
        assert_eq!(parse_directive(""), Some((0, Spelling::Fixed)));
        assert_eq!(parse_directive("up"), None);
    }

    #[test]
    fn signed_offsets() {
        assert_eq!(signed(2), "+2");
        assert_eq!(signed(-3), "−3");
        assert_eq!(signed(0), "0");
    }
}
