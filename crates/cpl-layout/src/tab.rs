//! Tablature staves and how they wrap. A staff is a run of consecutive
//! non-blank lines of a tab section, read as one unit: its rows share
//! columns, so a staff too wide for the content box breaks every row at the
//! same column, into systems stacked one under the next, the way printed tab
//! does. A break prefers a bar line, then any column that splits no fret
//! number or word, and a continuation system repeats the string names.
//! Column arithmetic only; the caller multiplies by the monospaced advance.

/// One staff of a tab section, verbatim, in character columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Staff {
    pub rows: Vec<Vec<char>>,
    /// The widest row, trailing whitespace excluded.
    pub columns: usize,
    /// Columns of the string-name prefix (`e|`) every staff row shares, which
    /// a continuation system repeats; 0 when the rows have none.
    pub prefix: usize,
    /// Columns past the prefix where every row that reaches them has a bar
    /// line and at least one row does. A system may end just after one.
    pub bars: Vec<usize>,
    /// Columns `c` past the prefix where a system may end before `c`
    /// without splitting a fret number, technique or word in any row.
    pub gaps: Vec<usize>,
}

/// One system of a staff: the rows as drawn, and the widest one's columns.
#[derive(Debug, Clone, PartialEq)]
pub struct System {
    pub rows: Vec<String>,
    pub columns: usize,
}

/// A character that separates tokens on a tab row: a fret number such as
/// `12`, a technique such as `3h5`, or a word is a run of anything else.
fn separator(c: Option<char>) -> bool {
    matches!(c, None | Some('-' | ' ' | '='))
}

/// The staves of a tab section's lines: blank lines separate them and are
/// otherwise dropped.
pub fn staves<S: AsRef<str>>(lines: &[S]) -> Vec<Staff> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in lines.iter().map(AsRef::as_ref) {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(Staff::new(&current));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        out.push(Staff::new(&current));
    }
    out
}

impl Staff {
    pub fn new(lines: &[&str]) -> Self {
        let rows: Vec<Vec<char>> = lines
            .iter()
            .map(|line| line.trim_end().chars().collect())
            .collect();
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let prefix = prefix(&rows);
        let at = |row: &Vec<char>, column: usize| row.get(column).copied();
        let bars = (prefix..columns)
            .filter(|&column| {
                let mut any = false;
                let all = rows.iter().all(|row| match at(row, column) {
                    Some('|') => {
                        any = true;
                        true
                    }
                    // A row that has ended, or a blank over the bar (a
                    // chord-name row above the staff), does not object.
                    None | Some(' ') => true,
                    _ => false,
                });
                all && any
            })
            .collect();
        let gaps = (prefix + 1..columns)
            .filter(|&column| {
                rows.iter()
                    .all(|row| separator(at(row, column - 1)) || separator(at(row, column)))
            })
            .collect();
        Self {
            rows,
            columns,
            prefix,
            bars,
            gaps,
        }
    }

    /// The staff broken into systems no wider than `max` columns where its
    /// breaks allow, as evenly as the fewest systems permit: eight bars that
    /// need two systems break four and four, not five and three. A system
    /// that is still wider holds a stretch with no break at all, and the
    /// caller reports it as too wide.
    pub fn systems(&self, max: usize) -> Vec<System> {
        let fewest = self.greedy(max);
        if fewest.len() < 2 {
            return fewest;
        }
        // The narrowest width that still needs no more systems.
        let mut best = fewest;
        for narrower in (self.prefix + 1..max).rev() {
            let systems = self.greedy(narrower);
            if systems.len() > best.len() {
                break;
            }
            best = systems;
        }
        best
    }

    /// Systems filled one after another, each as wide as `max` allows.
    fn greedy(&self, max: usize) -> Vec<System> {
        let mut out = Vec::new();
        let mut start = 0;
        while start < self.columns {
            let lead = if start == 0 { 0 } else { self.prefix };
            let room = max.saturating_sub(lead);
            let end = if self.columns - start <= room {
                self.columns
            } else {
                self.end_of_system(start, room)
            };
            out.push(self.system(start, end));
            start = end;
            // A break before blanks starts the next system at its content.
            while start < self.columns
                && self
                    .rows
                    .iter()
                    .all(|row| row.get(start).is_none_or(|c| *c == ' '))
            {
                start += 1;
            }
        }
        out
    }

    /// Where a system that starts at `start` and may take `room` columns
    /// ends: just after the last bar that fills at least half the room, else
    /// at the last gap, else at the last bar that fits, else at the first
    /// break past the room (too wide), else the staff's end.
    fn end_of_system(&self, start: usize, room: usize) -> usize {
        let limit = start + room;
        // Each system must show something past the repeated prefix.
        let least = start.max(self.prefix) + 1;
        let fits = |end: &usize| (least..=limit).contains(end);
        let bar = self.bars.iter().map(|b| b + 1).rev().find(fits);
        let gap = self.gaps.iter().copied().rev().find(fits);
        match (bar, gap) {
            (Some(bar), _) if 2 * (bar - start) >= room => bar,
            (_, Some(gap)) => gap,
            (Some(bar), None) => bar,
            (None, None) => self
                .bars
                .iter()
                .map(|b| b + 1)
                .chain(self.gaps.iter().copied())
                .filter(|end| *end > limit)
                .min()
                .unwrap_or(self.columns),
        }
    }

    fn system(&self, start: usize, end: usize) -> System {
        let rows: Vec<String> = self
            .rows
            .iter()
            .map(|row| {
                let mut text = String::new();
                if start > 0 {
                    text.extend(row.iter().take(self.prefix));
                    // A row shorter than the prefix pads to it.
                    text.extend(std::iter::repeat_n(
                        ' ',
                        self.prefix.saturating_sub(row.len()),
                    ));
                }
                text.extend(row.iter().take(end).skip(start));
                text.trim_end().to_owned()
            })
            .collect();
        let columns = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        System { rows, columns }
    }
}

/// The string-name prefix shared by every row of a staff: the columns up to
/// and including a first bar at the same column in each row, preceded by at
/// most three characters of name (`e|`, `D#|`, `E ||`'s first bar). A row
/// that is blank across those columns (chord names over the staff) agrees;
/// at least two rows must carry the bar.
fn prefix(rows: &[Vec<char>]) -> usize {
    let mut column = None;
    let mut barred = 0;
    for row in rows {
        match row.iter().position(|c| *c == '|') {
            Some(bar) if bar <= 3 && row[..bar].iter().any(|c| !c.is_whitespace()) => {
                if column.is_some_and(|column| column != bar) {
                    return 0;
                }
                column = Some(bar);
                barred += 1;
            }
            _ => {}
        }
    }
    let Some(column) = column else { return 0 };
    let blank_or_named = rows.iter().all(|row| {
        row.get(column) == Some(&'|') || row.iter().take(column + 1).all(|c| c.is_whitespace())
    });
    if barred >= 2 && blank_or_named {
        column + 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIFF: [&str; 6] = [
        "e|-----0-----|-----0-----|-----0-----|-----0-----|",
        "B|---1---1---|---1---1---|---3---3---|---1---1---|",
        "G|-----------|-2-------2-|-----------|-2-------2-|",
        "D|-2-------2-|-----------|-0-------0-|-----------|",
        "A|-3---------|-----------|-----------|-0---------|",
        "E|-----------|-3---------|-----------|-----------|",
    ];

    #[test]
    fn a_staff_is_the_run_of_lines_between_blanks() {
        let lines = ["", "e|--3--|", "B|--1--|", "", "", "Let ring", ""];
        let staves = staves(&lines);
        assert_eq!(staves.len(), 2);
        assert_eq!(staves[0].rows.len(), 2);
        assert_eq!(staves[1].rows.len(), 1);
    }

    #[test]
    fn string_names_are_the_prefix_and_bar_lines_are_found() {
        let staff = Staff::new(&RIFF);
        assert_eq!(staff.prefix, 2);
        assert_eq!(staff.columns, 50);
        assert_eq!(staff.bars, [13, 25, 37, 49]);
    }

    #[test]
    fn a_staff_that_fits_is_one_system_verbatim() {
        let staff = Staff::new(&RIFF);
        let systems = staff.systems(50);
        assert_eq!(systems.len(), 1);
        assert_eq!(systems[0].rows, RIFF);
    }

    #[test]
    fn a_wide_staff_breaks_every_row_after_the_same_bar_and_repeats_the_names() {
        let staff = Staff::new(&RIFF);
        let systems = staff.systems(30);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows[0], "e|-----0-----|-----0-----|");
        assert_eq!(systems[1].rows[0], "e|-----0-----|-----0-----|");
        assert_eq!(systems[1].rows[5], "E|-----------|-----------|");
        assert_eq!(systems[1].rows[1], "B|---3---3---|---1---1---|");
        assert!(systems.iter().all(|s| s.columns <= 30));
    }

    #[test]
    fn systems_balance_rather_than_leave_a_short_last_one() {
        let row = |name: char| {
            let mut row = format!("{name}|");
            for _ in 0..8 {
                row.push_str("--3--|");
            }
            row
        };
        let rows = [row('e'), row('B')];
        let staff = Staff::new(&rows.iter().map(String::as_str).collect::<Vec<_>>());
        // Five bars fit; eight bars over two systems go four and four.
        let systems = staff.systems(2 + 5 * 6);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].columns, systems[1].columns);
        assert_eq!(systems[0].rows[0], "e|--3--|--3--|--3--|--3--|");
    }

    #[test]
    fn without_a_bar_in_reach_it_breaks_between_dashes_never_inside_a_number() {
        let staff = Staff::new(&[
            "e|--12--10--8--7--5--3--2--0--|",
            "B|--------------------------3-|",
        ]);
        let systems = staff.systems(14);
        assert!(systems.len() > 1);
        for system in &systems {
            assert!(system.columns <= 14, "{system:?}");
        }
        // Every fret survives whole, in order, across the systems.
        let frets: Vec<String> = systems
            .iter()
            .flat_map(|s| {
                s.rows[0][2..]
                    .split(|c: char| !c.is_ascii_digit())
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(frets, ["12", "10", "8", "7", "5", "3", "2", "0"]);
    }

    #[test]
    fn chord_names_over_a_staff_break_with_it() {
        let staff = Staff::new(&[
            "  Am          C",
            "e|-----0-----|-----0-----|",
            "B|---1---1---|---1---1---|",
        ]);
        assert_eq!(staff.prefix, 2);
        let systems = staff.systems(16);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows[0], "  Am");
        assert_eq!(systems[1].rows[0], "  C");
        assert_eq!(systems[1].rows[1], "e|-----0-----|");
    }

    #[test]
    fn a_text_line_wraps_at_words() {
        let staff = Staff::new(&["Let the last chord ring while the band stops"]);
        let systems = staff.systems(20);
        assert!(systems.iter().all(|s| s.columns <= 20));
        let joined: Vec<String> = systems.iter().map(|s| s.rows[0].clone()).collect();
        assert_eq!(
            joined.join(" "),
            "Let the last chord ring while the band stops"
        );
    }

    #[test]
    fn an_unbreakable_stretch_overflows_rather_than_splitting() {
        let staff = Staff::new(&["e|-3h5p3-|", "B|-1h3p1-|"]);
        let systems = staff.systems(5);
        assert!(systems.iter().any(|s| s.columns > 5));
        assert!(
            systems
                .iter()
                .all(|s| !s.rows[0].contains("h5p") || s.rows[0].contains("3h5p3"))
        );
    }
}
