# App and page composition

`cpl-app` owns the current headless `App`, `Config`, and plain `Frame` data.
`App::new` takes a parsed `Song` and the host's text `Measure`. It keeps the
source song intact and measures it whole, tablature included. It measures the display song at the floor and larger whole-point
sizes up to the configured maximum. `frame` and `frame_scrolled` then take
only logical window dimensions and the scroll offset, and never measure, so
resize, scroll and polarity changes need no new CoreText measurement. The
macOS host owns `App` on a worker and submits viewport requests from the
window thread; only the newest complete result is accepted. `cpl-app` has no
window or GPU dependency.

The page uses up to four columns, `column_gutter` apart. The body follows
the Figma mock at 17 pt: a chord/lyric pair is `Page/Chord` (SF Pro
Semibold 15/16 pt, ink-soft) four points over `Page/Lyric` (17/25 pt,
ink), repeated every `line_height` (53 pt, the 45 pt pair plus 8);
a section's `Page/Label` (Bold 14/27 pt, tracked 8%, ink-soft, ink for a
chorus) sits `label_gap` above its first line, its baseline `label_baseline`
into its row; sections are 18 pt apart (`section_gap` plus the last line's
lead).

The status strip is `Page/Status` (SF Pro 12/21 pt, ink-soft) `status_drop`
above the bottom margin: where the page stands on the left (the mode,
the key the player reads with any live transpose, as `Bm +2`, then the
columns and size; a strip under 500 pt names the key only when
transposed), and the keys
for the page's mode at the right margin when both fit with two em between
them. The hints' widths are measured once per candidate body size in
`App::new`, so composing never measures. All page dimensions are in
`config/default.toml`; font sizes are points and the other values are in
body-size units except margins, which are points. `Config::with_overrides`
lays a user's `[page]` table over those defaults: the file names only the
keys it changes, and a key the defaults lack is an error. `body_size` is
the readability floor; `Config::with_floor` (the host's `--floor`) sets it
from 6 to 72 pt and raises `max_body_size` to meet it. `App::fit_solution`
weighs every ladder candidate that fits at the floor (`cpl_layout::candidates`)
by `Config::size_value` of its largest fitting size less
`Config::step_cost`, so text shrinks toward the floor before the page drops
diagrams, pills or the title; a tab fold the reader pinned costs nothing. A
page that fits at no candidate scrolls at `Config::scroll_size`, the
comfortable size, as does the search screen's strip. A song crossing the bottom
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
viewport. Timed scroll modes, per-step on/off/auto pins (other than the
tablature pin), and normalised input events in the proposal are not
implemented yet.

## Tablature

A shown tab section draws each system of its staves (see
[layout](layout.md)) as one group: its rows in the monospaced face at
`tab_size`, in the soft ink, `tab_inset` right of a faint rule `tab_rule`
wide that runs down the system's rows. Rows are `tab_row` apart, with the
baseline at `tab_baseline`, and systems are `tab_system_gap` apart. A
paged or scrolled view therefore never cuts a system. A folded tab draws its
label and a faint `· FOLDED` cue, or `FOLDED` in the column when the label
sits in the margin. `App::set_tabs` takes the reader's `Tabs` choice, drops
the prepared page, and applies the choice to the fit and to the scroll page.
`tabs_folded` tells the host whether this viewport folds a tab, so the `t`
key can unfold a folded tab or fold a shown one; a song without tablature
ignores `t`. `--tabs show|fold` opens the viewer, or `cpl render`, with the
choice pinned. The default config is embedded at
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

## Paged mode

A scrolling page also carries its page starts. Walking the groups in order,
each page begins at the top of the first group whose bottom falls past the
content box on the page before, so a page always opens on a whole
chord/lyric group, no line repeats or is skipped, and the last page may be
short. `App::page_view(width, height, page)` returns the same `ScrollView`
as `view` at that start (unclamped by `max_offset`, clamped to the last
page), with a status naming the section and `PAGE n / N`;
`App::page_count` is 1 for a song that fits. The host keeps the page index
and toggles the mode with `v`.

A scroll page lays its header out at the content width like any other. If
the wrapped title would take more than half the content box, the scroll page
drops it, as ladder step 7 would: the header is one scroll group, and a
group taller than the viewport could never be shown whole.

Frames carry `Shape`s beside `Text`: rounded rectangles in page points,
filled or outlined, with a group and an ink like text. Pills are an ink-soft
capsule outline with the label in soft ink and the value in strong ink, the
baseline 0.35 of the pill text size below the capsule's centre. A scroll
page's group bounds include its shapes, so a pill shows only with its header.

A diagram is composed by `App::compose_diagram`, after chordpro-pdf's
`drawDiagram`: the name centred over the grid, o and x over open and muted
strings, grid lines as thin filled shapes, a heavy nut (or the `Nfr` note
for a shape above it), a dot per fretted string, a capsule for each barre,
and finger numbers under the strings. Name, grid and dots are strong ink;
the marks are soft. The scroll page keeps its header under half the
viewport by dropping diagrams, then pills, then the title, the ladder's own
order.
