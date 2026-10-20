//! An ordered queue of songs with the current one and its neighbours measured
//! ahead of time, so a song change never waits on CoreText measurement.
//!
//! The host owns a `Setlist` on its page worker. `current` always has an
//! `App`; `preload` builds one missing neighbour per call so the worker can
//! interleave it with page requests, and `go_to` prunes everything further
//! than one song from the new position so memory stays bounded by three
//! measured songs however long the list is.

use crate::{App, Config, Measure, Polarity};
use cpl_song::Song;
use std::collections::BTreeMap;

pub struct Setlist {
    songs: Vec<Song>,
    current: usize,
    config: Config,
    polarity: Polarity,
    apps: BTreeMap<usize, App>,
}

impl Setlist {
    /// `songs` must not be empty; the first song is current.
    pub fn new(songs: Vec<Song>, config: Config, polarity: Polarity) -> Self {
        assert!(!songs.is_empty(), "a setlist needs at least one song");
        Self {
            songs,
            current: 0,
            config,
            polarity,
            apps: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }

    /// Zero-based index of the current song.
    pub fn position(&self) -> usize {
        self.current
    }

    pub fn title(&self, index: usize) -> &str {
        self.songs[index].first_meta("title").unwrap_or("Untitled")
    }

    /// The current song's app, measured now if it was not preloaded.
    pub fn current(&mut self, measure: &impl Measure) -> &mut App {
        let index = self.current;
        self.ensure(index, measure);
        self.apps.get_mut(&index).unwrap()
    }

    /// The app at `index` if it has been measured, without measuring it.
    pub fn ready(&self, index: usize) -> Option<&App> {
        self.apps.get(&index)
    }

    pub fn ready_mut(&mut self, index: usize) -> Option<&mut App> {
        self.apps.get_mut(&index)
    }

    /// Measures one missing neighbour, the next song first. Returns whether
    /// there was anything to do, so a worker can call it until it is idle.
    pub fn preload(&mut self, measure: &impl Measure) -> bool {
        let neighbours: Vec<usize> = self.neighbours().collect();
        for index in neighbours {
            if !self.apps.contains_key(&index) {
                self.ensure(index, measure);
                return true;
            }
        }
        false
    }

    /// Moves to `index` (clamped to the list) and forgets songs that are no
    /// longer within one of it. Returns the new position.
    pub fn go_to(&mut self, index: usize) -> usize {
        self.current = index.min(self.songs.len() - 1);
        let keep: Vec<usize> = self.neighbours().collect();
        self.apps.retain(|index, _| keep.contains(index));
        self.current
    }

    /// Moves by `delta` songs, stopping at either end rather than wrapping:
    /// on stage a wrap from the last song back to the first is a surprise.
    pub fn step(&mut self, delta: isize) -> usize {
        let target = (self.current as isize + delta).max(0) as usize;
        self.go_to(target)
    }

    /// Current song first, then next, then previous: the order preloading
    /// should fill them in.
    fn neighbours(&self) -> impl Iterator<Item = usize> + '_ {
        let current = self.current;
        [
            Some(current),
            current.checked_add(1),
            current.checked_sub(1),
        ]
        .into_iter()
        .flatten()
        .filter(move |index| *index < self.songs.len())
    }

    fn ensure(&mut self, index: usize, measure: &impl Measure) {
        if !self.apps.contains_key(&index) {
            let app = App::new(
                self.songs[index].clone(),
                self.config.clone(),
                self.polarity,
                measure,
            );
            self.apps.insert(index, app);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Face;
    use std::cell::Cell;

    /// Counts the songs measured through it by their first lyric line.
    struct Counting {
        calls: Cell<usize>,
    }
    impl Measure for Counting {
        fn width(&self, text: &str, size_pt: f32, _: u16, _: Face) -> f32 {
            self.calls.set(self.calls.get() + 1);
            text.chars().count() as f32 * size_pt * 0.5
        }
    }

    fn songs(titles: &[&str]) -> Vec<Song> {
        titles
            .iter()
            .map(|title| {
                cpl_song::parse(&format!(
                    "{{title: {title}}}\n{{start_of_verse: Verse 1}}\n[C]Only line of {title}\n{{end_of_verse}}"
                ))
                .song
            })
            .collect()
    }

    fn setlist(titles: &[&str]) -> Setlist {
        Setlist::new(songs(titles), Config::default(), Polarity::Light)
    }

    #[test]
    fn preloading_measures_the_next_song_before_it_is_asked_for() {
        let measure = Counting {
            calls: Cell::new(0),
        };
        let mut list = setlist(&["One", "Two", "Three"]);
        list.current(&measure);
        assert!(list.ready(0).is_some());
        assert!(list.ready(1).is_none());
        assert!(list.preload(&measure));
        assert!(list.ready(1).is_some(), "next song is measured first");
        assert!(
            !list.preload(&measure),
            "no previous song to measure at the start"
        );
        let measured_before_move = measure.calls.get();
        assert_eq!(list.step(1), 1);
        list.current(&measure);
        assert_eq!(
            measure.calls.get(),
            measured_before_move,
            "moving to a preloaded song measures nothing"
        );
    }

    #[test]
    fn moving_keeps_only_the_current_song_and_its_neighbours() {
        let measure = Counting {
            calls: Cell::new(0),
        };
        let mut list = setlist(&["One", "Two", "Three", "Four"]);
        list.current(&measure);
        while list.preload(&measure) {}
        assert_eq!(list.step(1), 1);
        while list.preload(&measure) {}
        assert!(list.ready(0).is_some() && list.ready(2).is_some());
        assert_eq!(list.step(1), 2);
        assert!(list.ready(0).is_none(), "two songs back is forgotten");
        assert!(list.ready(1).is_some() && list.ready(2).is_some());
        assert!(list.ready(3).is_none(), "not measured until preload runs");
        assert!(list.preload(&measure));
        assert!(list.ready(3).is_some());
    }

    #[test]
    fn stepping_stops_at_both_ends_instead_of_wrapping() {
        let mut list = setlist(&["One", "Two"]);
        assert_eq!(list.step(-1), 0);
        assert_eq!(list.step(5), 1);
        assert_eq!(list.step(1), 1);
        assert_eq!(list.title(list.position()), "Two");
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_song_without_a_title_is_listed_as_untitled() {
        let list = Setlist::new(
            vec![cpl_song::parse("[C]Hello").song],
            Config::default(),
            Polarity::Light,
        );
        assert_eq!(list.title(0), "Untitled");
    }
}
