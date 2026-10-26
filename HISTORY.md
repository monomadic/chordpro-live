# History

This is a record of implemented work and observed checks. `TASKS.md` holds
open work; `PROPOSAL.md` holds design decisions. Dates are local to the project
session. A passing headless test does not stand in for a visual window check.

## 2026-10-21

- **No more blank first window.** `CPL_TIMING` logs now carry a
  millisecond stamp from process start and record window creation, each
  accepted page and each present. They showed every launch skipping its
  first presents as `Occluded` while the window appears; a fast page
  (the inline sample, 7 ms) could be accepted inside that window, skipped,
  and never presented, leaving the window blank until a keypress — the two
  blank captures seen earlier. `Gpu::render` returns `Presented::{Yes,
  Skipped, Reconfigure}`; a skip is remembered and retried on
  `WindowEvent::Occluded(false)` and on a 100 ms idle tick, after which the
  loop sleeps on `ControlFlow::Wait` again. 20 captured first-launches after
  a rebuild (inline sample, Scarborough Fair at 980×720, sample page) all
  drew the page; 2 of them needed the retry. `scripts/launch-check.sh`
  automates the log half of that check. Workspace tests, the visual goldens
  and strict Clippy passed.
- **Visual regression goldens.** `tests/visual.rs` runs eleven `cpl render`
  cases and compares each with a committed golden in `tests/goldens/`,
  `sips`-compressed so the set is about 540 KB and every golden opens in Preview or
  on GitHub. `cpl-render/src/png.rs` gained a decoder (full inflate after
  puff.c, all five row filters) to read them back. Tolerance: a pixel differs
  when a channel moves more than 24 levels, a case fails past 0.02% of its
  pixels; a failing case leaves its render and a diff in `target/visual/`,
  and `CPL_BLESS=1` rewrites the goldens. Checked by shifting lyric baselines
  0.1 em (every chords-above case failed) and widening the chord gap (caught
  only once `testdata/crowded_chords.cho` was added, since no other case
  crowds chords). The first goldens showed the inline sample drawing only its
  title: the 28 pt title strip from the chromeless window had taken the room
  that sample was sized for, so its default window is now 300×228 pt, back at
  step 9, confirmed in a real window. Light and dark polarity and body sizes
  17, 18, 19, 26 and 28 pt were inspected in the goldens. Workspace tests and
  strict Clippy passed. While capturing, a fresh window stayed blank once;
  that became the next P1 task.
- **Real CoreText ink checked against page bounds; three defects fixed.**
  `tests/page_bounds.rs` now covers both testdata songs at five window sizes
  and the scroll end, and wrapped lines with long chords, at 1x and 2x. It
  found: (1) a title wider than the content box running into the margin — a
  title now wraps by words (`HeaderLayout::title_rows`, new `title_leading`
  config key), fit mode keeps a long title instead of dropping it at step 7
  for width alone, and a scroll page drops a title that would wrap past half
  the viewport, since a header taller than the viewport could never be shown
  whole; (2) side labels chosen at the floor carried to a larger body size
  where the fixed-point margin no longer held them, drawing a label at
  −21 pt — `layout` now marks such a candidate unfit; (3) measuring while
  another thread rasterised returned wrong advances in about one layout in
  ten — the shim is serialised behind a process-wide lock, and
  `tests/coretext_threads.rs` fails without it. `cpl render` writes the page
  the window would present to a PNG at any scale (a dependency-free encoder
  in `cpl-render/src/png.rs`); 1x and 2x renders of both songs, the scroll
  sample and a dark paged sample were inspected. Workspace tests and strict
  Clippy passed; the page-bounds test passed 30 of 30 parallel runs.
  Unverified: a real 1x window.
- **Paged mode.** `v` switches an overflowing song between scrolling and
  turning pages. `App::page_count` and `App::page_view` give the pages of
  the prepared scroll page: each begins at the top of the first chord/lyric
  group the page before could not hold whole (a group taller than a page
  starts its own page instead of looping), the last page may be short, and
  the status names the section and `PAGE n / N`. The host's worker now
  computes the view before choosing a slab, so a page turn is an offset
  over the retained slab and composes nothing unless it leaves the slab; a
  slab reaching the end of the page covers a short last page. Space also
  scrolls in scroll mode. `--paged` and `--page N` open paged for captures.
  Two new headless tests (every line once and in order across pages; a
  fitted song is one page); workspace tests and strict Clippy passed;
  pages 1, 2 and a clamped 9 of the scroll sample captured at 2x. Unverified:
  key-driven turns in a window.
- **Wide lyric lines wrap.** Measurement now records every word of a line
  with its width, and one pure row flow (`MeasuredLine::rows`) produces both
  the unwrapped runs and the wrapped rows: a break is allowed only after a
  word ending in whitespace, a chord stays over the first word of its
  segment, consecutive words of one segment merge back into a single run,
  and continuation rows start at `wrap_indent` (new config key, 1 em). The
  ladder's blocks wrap at the content width, each candidate counts its
  continuation rows, and the app takes a larger body size only when it wraps
  no more than the floor does. The scroll page tolerates a header wider than
  the window. `WIDTH NEEDED` survives only for an unbreakable run. Workspace
  tests (five new) and strict Clippy passed. Window check, 2026-10-22, at
  2x: the scroll sample at 280×420 pt wraps "falling on the road" and
  "bringing us back home" with Am and G following their words onto indented
  rows; Scarborough Fair at 420×720 pt fits at step 9 with "strand" wrapped.
  The capture showed the scroll status running off a 280 pt window, so under
  400 pt it now reads section, SCROLL and an arrow (one more test).
  Unverified: a 1x display, and wrapping at a column's own width.
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
