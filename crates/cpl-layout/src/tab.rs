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
    /// Which rows are staff lines (they carry a bar line `|`). The others
    /// are text written over or under the staff: chord names, counts,
    /// fingering notes. A staff with no bar line at all reads every row as a
    /// staff line.
    pub lines: Vec<bool>,
    /// The widest row, trailing whitespace excluded.
    pub columns: usize,
    /// Columns of the string-name prefix (`e|`) every staff line shares,
    /// which a continuation system repeats; 0 when the lines have none.
    pub prefix: usize,
    /// Columns past the prefix where every staff line that reaches them has
    /// a bar line and at least one does. A system may end just after one.
    pub bars: Vec<usize>,
    /// Columns `c` past the prefix where a system may end before `c`
    /// without splitting a fret number, technique or word on a staff line.
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
        let mut staff_lines: Vec<bool> = rows.iter().map(|row| row.contains(&'|')).collect();
        if !staff_lines.contains(&true) {
            staff_lines = vec![true; rows.len()];
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        let prefix = prefix(&rows, &staff_lines);
        let at = |row: &Vec<char>, column: usize| row.get(column).copied();
        let staff = || {
            rows.iter()
                .zip(&staff_lines)
                .filter(|(_, l)| **l)
                .map(|(r, _)| r)
        };
        let bars = (prefix..columns)
            .filter(|&column| {
                let mut any = false;
                let all = staff().all(|row| match at(row, column) {
                    Some('|') => {
                        any = true;
                        true
                    }
                    // A line that has ended, or a blank where the others
                    // have a bar, does not object.
                    None | Some(' ') => true,
                    _ => false,
                });
                all && any
            })
            .collect();
        let gaps = (prefix + 1..columns)
            .filter(|&column| {
                staff().all(|row| separator(at(row, column - 1)) || separator(at(row, column)))
            })
            .collect();
        let mut staff = Self {
            rows,
            lines: staff_lines,
            columns,
            prefix,
            bars,
            gaps,
        };
        // The bar of a second staff's string names, written beside the
        // first, is no place to break: the names belong to what follows.
        let names = |bar: &usize| (bar + 1).checked_sub(staff.prefix);
        let restated: Vec<usize> = (staff.bars.iter())
            .filter(|bar| names(bar).is_some_and(|start| start > 0 && staff.restates_prefix(start)))
            .copied()
            .collect();
        staff.bars.retain(|bar| !restated.contains(bar));
        staff
    }

    /// The staff broken into systems no wider than `max` columns where its
    /// breaks allow, as evenly as the fewest systems permit: eight bars that
    /// need two systems break four and four, not five and three. Evening
    /// out never trades a break at a bar line for one inside a bar. A system
    /// that is still wider holds a stretch with no break at all, and the
    /// caller reports it as too wide.
    pub fn systems(&self, max: usize) -> Vec<System> {
        let fewest = self.greedy(max);
        if fewest.len() < 2 {
            return self.draw(&fewest);
        }
        // The narrowest width that still needs no more systems and breaks
        // at as many bar lines.
        let at_bars = |ends: &[(usize, usize)]| {
            ends.iter()
                .filter(|(_, end)| *end < self.columns && self.bars.contains(&(end - 1)))
                .count()
        };
        let mut best = fewest;
        for narrower in (self.prefix + 1..max).rev() {
            let systems = self.greedy(narrower);
            if systems.len() > best.len() {
                break;
            }
            if at_bars(&systems) >= at_bars(&best) {
                best = systems;
            }
        }
        self.draw(&best)
    }

    fn draw(&self, spans: &[(usize, usize)]) -> Vec<System> {
        let mut after = 0;
        spans
            .iter()
            .map(|&(start, end)| {
                let system = self.system(after, start, end);
                after = end;
                system
            })
            .collect()
    }

    /// Where text over or under the staff is cut for a break before `end`:
    /// a word that starts at or past it goes to the next system. Such text
    /// is often lined up without the string names, so a word that starts on
    /// the bar line, or within their width before it, belongs to the bar
    /// after it.
    fn cut(&self, end: usize) -> usize {
        if end >= self.columns {
            usize::MAX
        } else if self.prefix == 0 {
            end
        } else {
            end.saturating_sub(self.prefix + 1)
        }
    }

    /// The words of the text rows, as each row's (start, end) columns.
    fn text(&self) -> impl Iterator<Item = (&Vec<char>, Vec<(usize, usize)>)> {
        self.rows
            .iter()
            .zip(&self.lines)
            .filter(|(_, line)| !**line)
            .map(|(row, _)| (row, words(row)))
    }

    /// Systems filled one after another, each as wide as `max` allows, as
    /// the column span each takes.
    fn greedy(&self, max: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start = 0;
        while start < self.columns {
            let lead = if start == 0 || self.restates_prefix(start) {
                0
            } else {
                self.prefix
            };
            let room = max.saturating_sub(lead);
            let end = if self.columns - start + self.overhang(self.columns) <= room {
                self.columns
            } else {
                self.end_of_system(start, room)
            };
            out.push((start, end));
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

    /// Whether every staff line already shows the string names at `start`,
    /// as when a second staff is written beside the first on the same rows.
    fn restates_prefix(&self, start: usize) -> bool {
        self.prefix > 0
            && self
                .rows
                .iter()
                .zip(&self.lines)
                .filter(|(_, l)| **l)
                .all(|(row, _)| row.get(start..start + self.prefix) == row.get(..self.prefix))
    }

    /// How far text over or under the staff runs past a break before `end`:
    /// a word that starts before the cut stays whole in its system.
    fn overhang(&self, end: usize) -> usize {
        let cut = self.cut(end);
        self.text()
            .flat_map(|(_, words)| words)
            .filter(|&(from, to)| from < cut && to > end)
            .map(|(_, to)| to - end)
            .max()
            .unwrap_or(0)
    }

    /// Where a system that starts at `start` and may take `room` columns
    /// ends: just after the last bar that fills at least half the room, else
    /// at the last gap, else at the last bar that fits, else at the first
    /// break past the room (too wide), else the staff's end. A break fits
    /// when the text it carries past itself fits too.
    fn end_of_system(&self, start: usize, room: usize) -> usize {
        let limit = start + room;
        // Each system must show something past the repeated prefix.
        let least = start.max(self.prefix) + 1;
        let fits = |end: &usize| *end >= least && end + self.overhang(*end) <= limit;
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

    /// The system spanning `start..end` of the staff lines, after a break
    /// before `after` (0 for the first).
    fn system(&self, after: usize, start: usize, end: usize) -> System {
        // A system of nothing but words written beside the staff (a
        // "repeat" note) is not a staff, so it takes no string names.
        let shown = || {
            (self.rows.iter().zip(&self.lines))
                .filter(|(_, l)| **l)
                .map(|(row, _)| &row[start.min(row.len())..end.min(row.len())])
        };
        let words_only = shown().all(|cells| !cells.iter().any(|c| matches!(c, '-' | '|' | '=')))
            && shown().any(|cells| {
                cells
                    .windows(2)
                    .any(|pair| pair.iter().all(|c| c.is_alphabetic()))
            });
        let lead = if start > 0 && !words_only && !self.restates_prefix(start) {
            self.prefix
        } else {
            0
        };
        let (from, to) = (if after == 0 { 0 } else { self.cut(after) }, self.cut(end));
        let rows: Vec<String> = self
            .rows
            .iter()
            .zip(&self.lines)
            .filter_map(|(row, &line)| {
                if line {
                    let mut text: String = row.iter().take(lead).collect();
                    // A row shorter than the prefix pads to it.
                    text.extend(std::iter::repeat_n(' ', lead - row.len().min(lead)));
                    text.extend(row.iter().take(end).skip(start));
                    let text = text.trim_end();
                    // A note keeps only the rows it is written on.
                    return (!(words_only && text.is_empty())).then(|| text.to_owned());
                }
                // Text keeps each word whole in the system where it falls,
                // at its column relative to the staff.
                let mut text: Vec<char> = Vec::new();
                for (ws, we) in words(row)
                    .into_iter()
                    .filter(|&(ws, _)| from <= ws && ws < to)
                {
                    let column = (ws + lead).saturating_sub(start).max(text.len());
                    text.resize(column, ' ');
                    text.extend(&row[ws..we]);
                }
                // Text with nothing in this system takes no row.
                (!text.is_empty()).then(|| text.into_iter().collect())
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

/// The words of a row of text, as (start, end) columns.
fn words(row: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (column, c) in row.iter().chain([&' ']).enumerate() {
        match (start, c.is_whitespace()) {
            (None, false) => start = Some(column),
            (Some(from), true) => {
                out.push((from, column));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The string-name prefix shared by every staff line: the columns up to and
/// including a first bar at the same column in each, preceded by at most
/// three characters of name (`e|`, `D#|`, `E ||`'s first bar). At least two
/// lines must carry it; text over or under the staff does not count.
fn prefix(rows: &[Vec<char>], lines: &[bool]) -> usize {
    let mut column = None;
    let mut barred = 0;
    for row in rows.iter().zip(lines).filter(|(_, l)| **l).map(|(r, _)| r) {
        match row.iter().position(|c| *c == '|') {
            Some(bar) if bar <= 3 && row[..bar].iter().any(|c| !c.is_whitespace()) => {
                if column.is_some_and(|column| column != bar) {
                    return 0;
                }
                column = Some(bar);
                barred += 1;
            }
            _ => return 0,
        }
    }
    match column {
        Some(column) if barred >= 2 => column + 1,
        _ => 0,
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
        assert!(systems.iter().any(|s| s.columns > 5), "{systems:?}");
        assert!(
            systems
                .iter()
                .all(|s| !s.rows[0].contains("h5p") || s.rows[0].contains("3h5p3"))
        );
    }

    // The staves below are shaped after real scraped tabs in the user's
    // library (4 Non Blondes, Crowded House, A Star Is Born, Ween).

    #[test]
    fn chord_names_from_the_first_column_keep_the_string_names_repeating() {
        let staff = Staff::new(&[
            "C             G",
            "e|-----0-----|-----0-----|",
            "B|---1---1---|---1---1---|",
        ]);
        assert_eq!(staff.prefix, 2);
        assert_eq!(staff.lines, [false, true, true]);
        let systems = staff.systems(16);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows, ["C", "e|-----0-----|", "B|---1---1---|"]);
        assert_eq!(systems[1].rows, ["  G", "e|-----0-----|", "B|---1---1---|"]);
    }

    #[test]
    fn a_count_under_the_staff_does_not_cost_the_string_names() {
        let staff = Staff::new(&[
            "e|-------3--|----0---|",
            "B|---3------|--3-----|",
            "1 + 2 + 3   1 e + a",
        ]);
        assert_eq!(staff.prefix, 2);
        let systems = staff.systems(13);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[1].rows[0], "e|----0---|");
        assert_eq!(systems[0].rows[2], "1 + 2 + 3");
        assert_eq!(systems[1].rows[2], " 1 e + a");
    }

    #[test]
    fn a_chord_name_across_a_bar_line_stays_whole_in_the_bar_it_starts_in() {
        let staff = Staff::new(&[
            "Csus2          C7(sus2)   C7sus4",
            "e|---3-------3----|---3--------3---|",
            "G|5--5--5-5--5--5-|3--5--5-3---3---|",
        ]);
        // The chord crosses the first bar line, which is still a bar.
        assert_eq!(staff.bars, [18, 35]);
        let systems = staff.systems(26);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows[0], "Csus2          C7(sus2)");
        assert_eq!(systems[0].rows[1], "e|---3-------3----|");
        assert_eq!(systems[1].rows[0], "         C7sus4");
        assert_eq!(systems[1].rows[1], "e|---3--------3---|");
        assert!(systems.iter().all(|s| s.columns <= 26), "{systems:?}");
    }

    #[test]
    fn evening_out_never_trades_a_bar_line_for_a_break_inside_a_bar() {
        // Bars of 12, 26 and 28 columns: the break after the second bar
        // leaves systems of 40 and 28; anything narrower splits a bar.
        let staff = Staff::new(&[
            "  .  . G Am",
            "e|------------|-------------------------|---------------------------|",
            "D|------------|-7-----------------7-7-\\-|-4-----2h4-----4--2--4---4-|",
        ]);
        let systems = staff.systems(52);
        assert_eq!(systems.len(), 2);
        assert!(systems[0].rows[1].ends_with('|'), "{systems:?}");
        // The chord names all fall in the first system.
        assert_eq!(systems[1].rows[0], "e|---------------------------|");
    }

    #[test]
    fn a_second_staff_beside_the_first_does_not_repeat_its_string_names() {
        let staff = Staff::new(&[
            "e|---0----0-----|        e|---x-----x-----|",
            "B|---2----0-----|        B|---5-----7-----|",
        ]);
        let systems = staff.systems(24);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows[0], "e|---0----0-----|");
        assert_eq!(systems[1].rows[0], "e|---x-----x-----|");
        assert_eq!(systems[1].rows[1], "B|---5-----7-----|");
        // Even when the second staff's names fall where the first ends.
        let systems = staff.systems(28);
        assert_eq!(systems[0].rows[0], "e|---0----0-----|");
        assert_eq!(systems[1].rows[0], "e|---x-----x-----|");
    }

    #[test]
    fn a_note_beside_the_staff_that_wraps_alone_takes_no_string_names() {
        let staff = Staff::new(&[
            "Gb|-----x-----2-2-0--| continue strumming",
            "Db|-2---2-----2-2-0--| then repeat",
            "Ab|-2---2-----0-0-0--|",
        ]);
        let systems = staff.systems(24);
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[0].rows[0], "Gb|-----x-----2-2-0--|");
        assert_eq!(systems[1].rows, ["continue strumming", "then repeat"]);
    }

    #[test]
    fn text_with_nothing_in_a_system_takes_no_row_there() {
        let staff = Staff::new(&[
            "  G",
            "e|-----0-----|-----0-----|",
            "B|---1---1---|---1---1---|",
        ]);
        let systems = staff.systems(16);
        assert_eq!(systems[0].rows.len(), 3);
        assert_eq!(systems[1].rows, ["e|-----0-----|", "B|---1---1---|"]);
    }
}
