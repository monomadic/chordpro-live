# Column fit

`cpl-layout` receives measured section blocks in body-size units. It packs
whole sections into up to four columns with the greedy rising-target method
from `chordpro-tui/internal/pdf/layout.go`. For each column count it computes
the largest body size allowed by the available width and height, then reports
the best split and whether that split meets the configured floor.

`cpl-app` currently calls the fitter with a maximum and floor equal to the
17 pt stand body size. It measures each section through the host's `Measure`
implementation and draws the chosen columns at that fixed size. If the full
section gap fails, it tries zero gap as ladder step 1. The app then tries
short labels in the side margin (step 2) and a single-line header (step 3).
The side-label choice is skipped when a label is too wide, so the collapsed
header can still be tried. Step 5 folds tablature to its label; steps 4 and 6
have no rendered content yet. Step 7 removes the title after the earlier
applicable choices fail.
Step 8 collapses a repeated chorus to its first line, and step 9 uses inline
chords with a shorter line block. The lyric face stays at the body floor in
every candidate. A song that still cannot fit vertically is laid out as one
long column, with whole chord/lyric line groups selected for the scroll
viewport. Width overflow still needs a solution. Point-size presets and
paged mode remain unimplemented.
