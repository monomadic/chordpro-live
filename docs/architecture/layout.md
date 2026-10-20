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

Steps 4 and 6 (diagrams, metadata pills) have nothing to remove yet. A side
label beside a section with every line hidden keeps one row. A song with no
sections fits. The app selects the ladder step at the floor, then checks
larger premeasured sizes in descending order and draws the largest that fits.
Each candidate is checked using font advances measured at its actual point
size. A viewport's prepared scroll page stays at the floor. Point-size
presets and paged mode are still to come.
The app currently omits tablature from the display song before measuring, so
step 5 is inactive for the macOS viewer while the source model retains the tab.
