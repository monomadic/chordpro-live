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
header can still be tried. Step 7 removes the title after the earlier
applicable choices fail; steps 4–6 have no rendered content yet.
A song that still cannot fit shows a paging-needed message. Later reduction
steps, point-size presets, scroll and page fallback remain unimplemented.
