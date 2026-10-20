# History

This is a record of implemented work and observed checks. `TASKS.md` holds
open work; `PROPOSAL.md` holds design decisions. Dates are local to the project
session. A passing headless test does not stand in for a visual window check.

## 2026-10-21

- **Scrolling over a retained page, and a chromeless window.** The worker
  now retains each song's positioned page per viewport and composes a slab of
  it up to three viewports tall; `cpl_app::App::page` and `App::view` give the
  retained frame and, per step, the offset, the band of complete chord/lyric
  groups and the status text. The shader places the slab by a `View` uniform,
  paints paper outside the band and reads a separate one-line status texture,
  so a scroll step inside the slab uploads nothing but that strip; a step that
  leaves the slab composes the next slab from the retained frame without
  measuring. The next song's page and slab are preloaded the same way. The
  macOS window is chromeless with winit's own attributes (transparent
  titlebar, hidden title, full-size content view, no shadow, which on macOS 26
  is what removes the window contour); the page runs under a 28 pt title
  strip of paper so the traffic lights float over it, and `q` quits beside
  Escape. `--scroll N` opens a song scrolled, for captures. Headless tests
  check that a view's band contains exactly what `frame_scrolled` drew, that a
  fitted page views as itself, that a slab equals the matching rows of the
  whole page, and the uniform layout; workspace tests and strict Clippy
  passed; the fitted, scrolled (0 and 120 pt) and dark pages were captured at
  2x. Measured in a debug build: slab 50–120 ms, view 2 µs (fit) / 3–9 ms
  (scroll status strip). Unverified: a key-driven scroll in a window, release
  timings, and the window on a 1x display.
- **Setlists with the next song preloaded.** `cpl --view` now takes several
  songs or a folder (ChordPro extensions, name order) and opens them as a
  setlist; `n`/`p` and ← → move through it without wrapping, and the window
  title carries the song's position. `cpl_app::Setlist` keeps the current
  song and its two neighbours measured and drops anything further away; the
  page worker measures a missing neighbour or composes the next song's first
  page at the last viewport whenever it has no request waiting, one unit of
  work between checks for requests. A song change is an ordinary generation-
  stamped request, so the old page stays up until the new song's complete
  page arrives, and when the preloaded page matches the viewport and
  polarity it is sent without measuring or rasterising. `--song N` opens on
  a given song so a capture needs no keypress; `scripts/window-id.swift`
  finds the window for `screencapture -l`. Headless tests cover preload
  order, neighbour pruning, non-wrapping steps and folder expansion;
  workspace tests and strict Clippy passed; both songs of the two-song
  testdata setlist were captured at 980×720. Unverified: the key-driven
  swap in a window, and the latency of a swap on a long song.

- **Epic closed: P0 — make the song page usable.** Six tasks, all checked
  against the supplied “What's Up” source in a 1960×1504 px Retina window
  (`--view <song> --size 980x720`): tablature blocks hidden from the page but
  kept in the parsed song and the file; the screenshot reproduced at a fixed
  window size with focused regression tests for hidden tab, chord-only Intro,
  section context and post-tab text; the first viewport coherent, with no
  orphaned chord row at startup or after Page Down; chord-only lines drawn as
  compact rows; the title's quotation marks confirmed as authored; text after
  a hidden tab kept visible, since the viewer cannot tell a legend from an
  instruction. Unverified: a 1x window, and songs other than the supplied one.

- **Prepared complete pages on a worker.** The macOS callback now sends
  viewport requests; a worker owns song measurement, layout and CoreText page
  composition. A generation check discards stale resize or scroll results,
  and the previous texture remains visible until a completed replacement is
  uploaded. Workspace tests and strict Clippy passed. The supplied song was
  inspected in the macOS preview at startup, after Page Down, and after a
  polarity switch. Next-song preloading and scroll upload reduction remain
  open.
- **Reproducible song window size.** `--view <song> --size 980x720` sets a
  logical-point window size for the real-song visual check. The supplied song
  was inspected again in a 1960×1504 px Retina window. The first screen keeps
  Intro, Verse 1, and chord/lyric pairs within the page. A synthetic regression
  test confirms that ordinary text after a hidden tab remains visible: the
  viewer cannot reliably infer that such text is a tab legend. Put tab-only
  notes inside the tab environment in authored source.

## 2026-10-20

- **Checked the supplied “What's Up” source in the macOS window.** At a
  1960×1504 px Retina capture, the initial scroll position shows Intro,
  its compact chord row, and Verse 1 lyric/chord pairs without the tab block.
  After Page Down, pairs remain together and the status names CHORUS when its
  label is above the viewport. The title's quotation marks are authored in
  the ChordPro file. A notation legend outside the tab block remains visible
  as ordinary text; a fixed-size repeatable capture was added on 2026-10-21.
- **Measured ink against page bounds.** A macOS integration test now rasterises
  every positioned item on fitted pages through CoreText at 1x and 2x and
  checks its actual ink against the content bounds. The tested title, byline,
  labels, lyrics, and long chords stayed inside. This is a focused regression
  check, not a claim about every song or font fallback.
- **Scroll status retains section context.** The status strip now names the
  section containing the top visible song line, including after a label
  scrolls away. A focused headless test passed, and the built-in scroll
  sample was inspected in the macOS preview at its initial position and after
  Page Down. The user's “What's Up” source is still needed for its exact
  screenshot regression.
- **Chord-only progressions use a compact row.** Lines without sung words now
  keep normal-size chords and avoid drawing an empty lyric run, including at
  the inline reduction step. Headless layout and page tests passed; the
  affected real song still needs a window capture.
- **Tablature hidden from the macOS performance page.** `cpl-app` now measures
  and fits a display song without `Tab` sections while retaining the complete
  parsed song. Headless tests cover fit and scroll output. This responds to a
  user screenshot where a long tab dominated the page; the corrected window
  has not yet been captured with that source song.
- `22642aa` — added point-size candidates from the 17 pt floor to a
  configurable 28 pt maximum, measured at their actual drawn sizes. Workspace
  tests and strict Clippy passed. Visual verification was still outstanding.
- `c3906be` — reused the prepared scroll page and cached CoreText coverage by
  text, face, weight, size, and display scale. Scrolling still composites and
  uploads the visible page texture.
- `b8bd4bf` — included header width in the fit decision, so an overflowing
  title cannot be accepted as a fitting page.

Earlier implementation and design history is available in Git. New entries
should state what shipped, what was checked, and what remains unverified.
