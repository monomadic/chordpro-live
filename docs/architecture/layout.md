# Column fit

`cpl-layout` measures a song with `measure_song` into a `MeasuredSong` at
each candidate size, from the body floor to the configured maximum:
the header, each section's upper-cased label, and for every line two run
lists with em offsets and widths, one with chords above the lyric and one
with inline chords. A chord wider than its syllable pushes the next lyric past
the chord and its gap, so every chord starts exactly over the syllable it names. It is the only stage
that calls the host's `Measure`. A run's `Role` fixes its face, weight and em
size through `Metrics::style`, so measurement and drawing cannot disagree.
Text is measured at the body size rather than an abstract 1 em, because
system-font tracking changes with point size. Equal strings in one role are
measured once per candidate.
Measured lines distinguish chord-only progressions from sung lines. A
progression keeps its full chord runs at step 9 and contributes the compact
inline row height to its section block.

Section blocks are built from those widths in body-size units. `fit` packs
whole sections into up to four columns with the greedy rising-target method
from `chordpro-tui/internal/pdf/layout.go`. For each column count it computes
the largest body size allowed by the available width and height, then reports
the best split and whether that split meets the configured floor.

The reduction ladder lives here too. `Options` holds one choice per step, and
its methods give the em geometry each choice implies: the header layout,
each section's block, visible lines and line height. `layout` lays out one
candidate; `solve` tries the full page, then each `LADDER` step in order,
cumulatively, and returns the first that fits or the last one tried. Each
step carries an `applies` test, so a step that cannot change this song's
page (no tablature, no repeated chorus, a header too wide for one line,
labels too wide for the margin, a single section) is skipped rather than
tried, and never leaves a failing option on for the steps after it:

1. section gaps close; 2. labels move to the side margin; 3. title and
byline share one line; 5. tablature folds to its label; 7. the title drops;
8. a repeated chorus keeps only its first line; 9. chords go inline.

Step 4 drops the chord diagrams and step 6 the metadata pills. A side
label beside a section with every line hidden keeps one row. A song with no
sections fits. The app selects the ladder step at the floor, then checks
larger premeasured sizes in descending order and draws the largest that fits.
Each candidate is checked using font advances measured at its actual point
size. A viewport's prepared scroll page stays at the floor. Point-size
presets and paged mode are still to come.
`solve_with` takes the reader's `Tabs` choice: `Auto` lets step 5 fold
tablature, while `Show` and `Fold` pin it and skip step 5.

## Tablature

`measure_song` reads a tab section's lines into **staves** (`tab.rs`):
blank lines separate them, and each staff's rows share columns. It also
measures the monospaced column advance at `tab_size` once per section.
`Staff::new` finds the string-name prefix every row shares (`e|`, a blank
chord-name row agreeing), the **bar** columns where every row that reaches
them has `|`, and the **gaps** where a cut splits no fret number, technique
or word in any row. `Staff::systems(max)` breaks a staff wider than `max`
columns into systems: each ends after the last bar that fills at least half
its room, otherwise at the last gap, and a continuation repeats the prefix.
It then narrows the width while the system count holds, so eight bars over
two systems break four and four. A stretch with no break at all stays whole
and overflows, which the fit reports as unfit like any unbreakable run.
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
comments wrap by words. `Options::block` wraps at the viewport width and
`layout` counts each candidate's continuation rows in `Solution::wrapped_rows`;
the app accepts a larger body size only while it wraps no more rows than the
floor. A row wider than the limit means an unbreakable token, and the fit
reports it as not meeting the floor — the only case left for the
width-needed state. Wrapping at a column's own width, to allow more columns,
is not attempted yet.

A title wider than the content box wraps the same way, by its measured words
(`Header::title_tokens`): `Options::header` takes the content width and
returns `title_rows` with their baselines, `title_leading` title sizes apart,
and the header's real `width`. Fit mode therefore keeps a long title and
drops it at step 7 only when height requires. A single title word wider than
the box still overflows and makes the candidate unfit.

Side labels sit in the left margin, whose width is fixed in points, so the
room they have in em shrinks as the body size grows. `layout` marks a
candidate with side labels unfit when any label is wider than that room at
the candidate's own metrics, so a larger size never reuses side labels that
no longer fit.

**Metadata pills.** `measure_song` measures a pill for each of key, capo,
tempo and time the song states (`Header::pills`, label and value as
separate runs). `Options::header` places them right-aligned on the header's
last line, centred on its x-height, when the title block plus two pill gaps
leaves room for the whole row; otherwise they flow in rows of `meta_row`
height under the byline, wrapping at the content width. Ladder step 6 sets
`hide_pills`. The geometry (`pill_height`, `pill_pad`, `pill_gap`,
`pill_kv_gap`, `meta_row`) comes from chordpro-pdf's layout table.

**Chord diagrams.** `measure_song` measures a diagram for each chord the
song uses that has a shape (`Header::diagrams`: name, shape, and an `Nfr`
note for a shape above the nut), plus one mark width (the mark face is
monospaced). `Options::header` lays the cells out in rows under the pills,
`diagram_top_gap` below the header's last line, each cell
`PageMetrics::diagram_width` wide and `diagram_height` tall, wrapping at the
content width. Step 4 sets `hide_diagrams`. The geometry is chordpro-pdf's.
