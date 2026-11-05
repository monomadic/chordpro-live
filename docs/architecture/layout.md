# Column fit

`cpl-layout` measures a song with `measure_song` into a `MeasuredSong` at
one set of metrics; the app calls it once per candidate size, from the body
floor to the configured maximum. A `MeasuredSong` holds
the header, each section's upper-cased label, and for every line two run
lists with em offsets and widths, one with full-size chords and one with smaller chords, both above the lyrics. Lyrics retain their natural spacing, including across mid-word chord changes.
Chords start above their syllable; a crowded chord moves right only far enough
to clear the previous chord and its gap, without moving any lyric. It is the only stage
that calls the host's `Measure`. A run's `Role` fixes its face, weight and em
size through `Metrics::style`, so measurement and drawing cannot disagree.
Text is measured at the body size rather than an abstract 1 em, because
system-font tracking changes with point size. Equal strings in one role are
measured once per candidate.
Measured lines distinguish chord-only progressions from sung lines. A
progression keeps its full chord runs at step 7 and contributes the compact
row height to its section block.

Section blocks are built from those widths in body-size units. `fit` packs
whole sections into up to `max_columns` columns with the greedy
rising-target method from `chordpro-tui/internal/pdf/layout.go`. For each
column count it computes the largest scale (at most 1) the available width
and height allow at the candidate's body size, then reports the best split
and whether it reaches 1. The app centres the fitted column group using the
measured column widths and gutters. The title, byline, pills and diagrams
use fixed point sizes, and the fitter converts their dimensions to body
units when budgeting room for the song. Comments use a heavier weight and
extra space when they appear within a section.

The reduction ladder lives here too. `Options` holds one choice per step, and
its methods give the em geometry each choice implies: the header layout,
each section's block, visible lines and line height. `layout` lays out one
candidate; `solve` tries the full page, then each `LADDER` step in order,
cumulatively, and returns the first that fits or the last one tried. Each
step carries an `applies` test, so a step that cannot change this song's
page (a single section, labels too wide for the margin, no chord diagrams,
no tablature, no repeated chorus, no pills) is skipped rather than
tried, and never leaves a failing option on for the steps after it:

1. section gaps halve; 2. labels move to the side margin; 4. the chord
diagrams drop; 5. tablature folds to its label; 6. a repeated chorus keeps
only its first line; 7. chord rows get smaller; 8. the pills drop; 9. the title
drops. The header goes last. There is no step 3: the header always keeps
the byline on its own line (see PROPOSAL.md). A side
label beside a section with every line hidden keeps one row. A song with no
sections fits. `candidates` lists the full page and each applicable step,
cumulatively; the app draws each one that fits at the floor at the largest
premeasured size that keeps it, and weighs size against what the steps cost
(see [app](app.md)). Each candidate is checked using font advances measured
at its actual point size. A page that fits nowhere scrolls at the
comfortable size.
`solve_with` takes the reader's `Tabs` choice: `Auto` lets step 5 fold
tablature, while `Show` and `Fold` pin it and skip step 5.

## Tablature

`measure_song` reads a tab section's lines into **staves** (`tab.rs`):
blank lines separate them, and each staff's rows share columns. It also
measures the monospaced column advance at `tab_size` once per section.
`Staff::new` sorts the rows into **staff lines**, which carry a `|`, and
**text** written over or under them (chord names, counts, fingering), which
does not; a staff with no `|` at all reads every row as a staff line. It
finds the string-name prefix every staff line shares (`e|`), the **bar**
columns where every staff line that reaches them has `|`, and the **gaps**
where a cut splits no fret number, technique or word on a staff line. The
string-name bar of a second staff written beside the first on the same rows
is not a bar to break at. `Staff::systems(max)` breaks a staff wider than
`max` columns into systems: each ends after the last bar that fills at least
half its room, otherwise at the last gap. It then narrows the width while
the system count holds and no bar-line break turns into one inside a bar,
so eight bars over two systems break four and four. A stretch with no break
at all stays whole and overflows, which the fit reports as unfit like any
unbreakable run.

A continuation system repeats the prefix on its staff lines and pads text
rows to it, unless its staff lines already start with the names (the
second staff beside the first) or hold only a note written beside the staff
(`continue strumming`), which keeps just the rows it is written on. Text
never splits: each word goes to the system where it starts, and a word that
starts on the bar line or within the prefix's width before it goes to the
next, because scraped tabs often line text up without the string names. A
word that crosses a break stays whole, and the break must leave room for it.
Text with no word in a system takes no row there.
`Options::tab_systems` breaks every staff at the content width beside
`tab_inset`. A shown tab block is `tab_row` per row plus `tab_system_gap`
between systems. `groups` counts one scroll group per system.
`wrapped_rows` counts systems beyond one per staff. A folded tab
(`tab_folded`) keeps its label plus the `FOLDED` cue, and its block is
wide enough for both.

## Wrapping

A line wider than the content box wraps. `measure_song` records every word
of a lyric (with its trailing whitespace) as a measured `Token`, and
`MeasuredLine::rows(inline, max_width, indent)` flows the segments into rows:
a break may follow a token that ends in whitespace and never falls inside a
word or at a chord's syllable split (`Or[F]leans`), a chord stays over the
first word of its segment, consecutive words of one segment merge into one
run, and continuation rows start at `PageMetrics::wrap_indent`. The
unwrapped `above` and `inline` runs are the one-row flow of the same
function, so the two cannot disagree. A word that a chord splits
(`dis[E]courteously`) is one unit: the row breaks before it when the whole
word would not fit, never inside it. Tab lines wrap as staves (below);
comments wrap by words. An annotation (`[*rit.]`) is measured and drawn as
a chord is. A grid row is one unwrapped token in the tab face, its cells
padded to their column's widest so rows of one shape align; grids and text
blocks always take the compact rows (`Options::inline`), having no
chord row to keep. Embedded notation is not measured, so it takes no
space. `Options::block` wraps at the viewport width and
`layout` counts each candidate's continuation rows in `Solution::wrapped_rows`;
the app accepts a larger body size only while it wraps no more rows than the
floor. A row wider than the limit means an unbreakable token, and the fit
reports it as not meeting the floor — the only case left for the
width-needed state.

A title wider than the content box wraps the same way, by its measured words
(`Header::title_tokens`): `Options::header` takes the content width and
returns `title_rows` with their baselines, `title_leading` title sizes apart,
and the header's real `width`. Fit mode therefore keeps a long title and
drops it at step 9, the last, only when height requires. A single title word wider than
the box shrinks the whole title (`HeaderLayout::title_scale`) until it fits,
but never below the body size; past that it overflows and makes the
candidate unfit.

**Header geometry** is the Figma mock's `Header` frame at body 17, in em:
the first title baseline at `title_baseline`, wrapped title rows
`title_leading` title sizes apart, the byline (the artist and `{subtitle}`
joined by `BYLINE_SEPARATOR`) `byline_leading` byline sizes under the
title, and a hairline (`HeaderLayout::rule`) `header_rule_drop` under the
block's last baseline. Diagrams and the body start below the rule.

Side labels sit in the left margin, whose width is fixed in points, so the
room they have in em shrinks as the body size grows. `layout` marks a
candidate with side labels unfit when any label is wider than that room at
the candidate's own metrics, so a larger size never reuses side labels that
no longer fit.

**Metadata pills.** `measure_song` measures a pill for each of key, capo,
tempo and time the song states (`Header::pills`, label and value as
separate runs, each widened by `meta_tracking` per character, since the
text is letter-spaced). `Options::header` places them right-aligned,
centred `pill_rise` above the header's last baseline (the mock's pills sit
on the byline's text box), when the title block plus two pill gaps leaves
room for the whole row; otherwise they flow in rows of `meta_row` height
under the byline, wrapping at the content width. Ladder step 8 sets
`hide_pills`. The pill follows the mock: `Page/Meta` labels in SF Pro Heavy
and `MetaValue` values in Medium, both tracked 8%, in a full-round pill
`pill_height` tall whose `pill_stroke` outline (ink-soft) sits inside its
`pill_pad`, `pill_kv_gap` between label and value and `pill_gap` between
pills.

**Chord diagrams.** `measure_song` measures a diagram for each chord the
song uses that has a shape (`Header::diagrams`: name, shape, and an `Nfr`
note for a shape above the nut), plus one mark width (the mark face is
monospaced). `Options::header` lays the cells out in rows under the pills,
`diagram_top_gap` below the header rule, each cell
`PageMetrics::diagram_width` wide and `diagram_height` tall, wrapping at the
content width. Step 4 sets `hide_diagrams`. The geometry is that of
`chordpro-tui/internal/pdf/layout.go`.
