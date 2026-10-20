# App and page composition

`cpl-app` owns the current headless `App`, `Config`, and plain `Frame` data.
The macOS host passes a parsed `Song`, logical window dimensions, and a text
`Measure` implementation. `cpl-app` has no window or GPU dependency.

The page uses up to four stand-size columns. Chords and lyrics share a
line block: the chord baseline is immediately above the lyric baseline, and
section labels have their own gap. All page dimensions are in
`config/default.toml`; font sizes are points and the other values are in
body-size units except margins, which are points. If a line crosses the right
or bottom margin, this initial view shows a paging-needed message instead of
drawing a lyric below the readability floor.

The first three fit candidates run in proposal order: remove inter-section
gaps; move section labels into the side margin when all labels fit there;
and put the title and byline on one baseline if they fit the page width.
When those fail, step 7 removes the title; steps 4–6 concern diagrams,
tablature and metadata pills that this viewer does not render yet. A side
label candidate is skipped when its text will not fit in the margin.
Step 8 keeps the first line of a repeated chorus (exact line/segment
equality), and step 9 places small chord symbols inline before their lyric
segments. Every candidate retries at the same body size. The scroll and
page modes, later parser-dependent reductions, and normalised input events
in the proposal are not implemented yet. The default config is embedded
at build time; hot reload has not landed.
