# App and page composition

`cpl-app` owns the current headless `App`, `Config`, and plain `Frame` data.
`App::new` takes a parsed `Song` and the host's text `Measure`. It keeps the
source song intact but excludes tablature sections from the measured display
song for now. It measures the display song at the floor and larger whole-point
sizes up to the configured maximum. `frame` and `frame_scrolled` then take
only logical window dimensions and the scroll offset, and never measure, so
resize, scroll and polarity changes need no new CoreText measurement. The
macOS host owns `App` on a worker and submits viewport requests from the
window thread; only the newest complete result is accepted. `cpl-app` has no
window or GPU dependency.

The page uses up to four columns. Chords and lyrics share a
line block: the chord baseline is immediately above the lyric baseline, and
section labels have their own gap. All page dimensions are in
`config/default.toml`; font sizes are points and the other values are in
body-size units except margins, which are points. A song crossing the bottom
margin scrolls at the same body size. Width overflow still needs a solution.
Chord-only progressions sit on one compact row at full chord size. Whitespace
used to separate their chords is measured for alignment but not drawn as
an empty lyric run.

`frame` asks `cpl_layout::solve` for the least-reduced page that fits the
content box (inside the margins, above the status strip) and composes it in
points; the ladder order and its skip rules are in [layout](layout.md). An
unfit candidate draws no lyrics. When fit fails, `frame_scrolled` lays the
full song out as one column of unbounded height, falling back to inline
chords only if a line is too wide, and selects complete chord/lyric line
groups for the viewport. The macOS host owns the offset and handles arrow,
Page Up/Down, Home, and End keys. The scroll status names the section
containing the first visible song line, even after its label leaves the
viewport. Paged and timed scroll modes, diagram and
pill reductions, per-step on/off/auto pins, and normalised input events in
the proposal are not implemented yet. The default config is embedded at
build time; hot reload has not landed.
