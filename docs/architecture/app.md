# App and page composition

`cpl-app` owns the current headless `App`, `Config`, and plain `Frame` data.
The macOS host passes a parsed `Song`, logical window dimensions, and a text
`Measure` implementation. `cpl-app` has no window or GPU dependency.

The first page is a single-column stand view. Chords and lyrics share a
line block: the chord baseline is immediately above the lyric baseline, and
section labels have their own gap. All page dimensions are in
`config/default.toml`; font sizes are points and the other values are in
body-size units except margins, which are points. If a line crosses the right
or bottom margin, this initial view shows a paging-needed message instead of
drawing a lyric below the readability floor.

The fit ladder, balanced columns, scroll and page modes, and normalised input
events in the proposal are not implemented yet. The default config is embedded
at build time; hot reload has not landed.
