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
margin scrolls at the same body size. A line wider than the content box
wraps (see [layout](layout.md)); only an unbreakable run reaches the
width-needed state, and a scrolling page clips a too-wide title rather than
withholding the song.
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

## Setlist

`Setlist` is the ordered song queue, headless like the rest of the crate.
It owns the parsed songs and a map of measured `App`s for the current song
and its immediate neighbours: `current` measures on demand, `preload` measures
one missing neighbour per call (next before previous) so a worker can slot it
between requests, and `go_to`/`step` move without wrapping and drop any
`App` more than one song away, so memory is bounded by three measured songs.
The host keeps the song index and the scroll offset; a song change is a page
request like any other, which is what lets the old page stay visible until
the new song's complete page replaces it.

## Retained page and view

`App::page` returns the frame to retain for a viewport: a fitted page at
window height with its status strip, or the whole scrolled song as one tall
frame in page coordinates without one. `App::view` answers a scroll step from
the same prepared page with no measurement and no composition: the clamped
offset, the window-space band covering exactly the complete chord/lyric
groups `frame_scrolled` would draw (with 0.15 em of slack for ascenders and
descenders, safe because neighbouring groups keep 0.45 em clear), and the
status text. The renderer masks outside the band, so the retained page can
be drawn at any offset while partial lines stay hidden. `frame_scrolled`
remains for headless checks and shares the status text with `view`.
