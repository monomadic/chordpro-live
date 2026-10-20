# App and page composition

`cpl-app` owns the current headless `App`, `Config`, and plain `Frame` data.
`App::new` takes a parsed `Song` and the host's text `Measure`, and measures
the song once. `frame` and `frame_scrolled` then take only logical window
dimensions and the scroll offset, and never measure, so resize, scroll and
polarity changes are arithmetic. `cpl-app` has no window or GPU dependency.

The page uses up to four stand-size columns. Chords and lyrics share a
line block: the chord baseline is immediately above the lyric baseline, and
section labels have their own gap. All page dimensions are in
`config/default.toml`; font sizes are points and the other values are in
body-size units except margins, which are points. A song crossing the bottom
margin scrolls at the same body size. Width overflow still needs a solution.

The first three fit candidates run in proposal order: remove inter-section
gaps; move section labels into the side margin when all labels fit there;
and put the title and byline on one baseline if they fit the page width.
When those fail, step 5 folds parsed tablature to its label; step 7 removes
the title. Steps 4 and 6 concern diagrams and metadata pills that this viewer
does not render yet. A side label candidate is skipped when its text will
not fit in the margin.
Step 8 keeps the first line of a repeated chorus (exact line/segment
equality), and step 9 places small chord symbols inline before their lyric
segments. Every candidate retries at the same body size. When fit fails,
`frame_scrolled` lays out the full song as one column and selects complete
chord/lyric line groups for the viewport. The macOS host owns the offset and
handles arrow, Page Up/Down, Home, and End keys. Paged and timed scroll modes,
diagram and pill reductions, and normalised input events in the proposal
are not implemented yet. The default config is embedded
at build time; hot reload has not landed.
