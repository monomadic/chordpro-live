# Tasks

Ordered by impact on a singer using the macOS viewer. `PROPOSAL.md` holds the
product decisions; this file tracks the work needed to make the current build
meet them. A task is complete only after a real window check and the relevant
automated checks. When every task in an epic is checked, the epic moves to
`HISTORY.md` (see AGENTS.md, "Working from the task list").

## P1 — dependable performance behavior

- [x] Move song measurement and page preparation off the window callback.
  A worker measures, lays out and rasterises complete pages; stale results
  are discarded and resize keeps the previous texture until replacement.
- [x] Preload the next song and swap only complete pages during setlist
  navigation. `cpl --view a.cho b.cho` or `--view <folder>` opens a setlist;
  `Setlist` in `cpl-app` keeps the current song and its neighbours measured,
  the worker composes the next song's first page while idle, and `n`/`p`
  swap to it only when that complete page is accepted. Both songs of the
  two-song testdata setlist were captured through `--song`; the key-driven
  swap itself has not been captured.
- [x] Replace full page recomposition and texture upload on every scroll step
  with retained drawing or a moving viewport over a prepared page. The worker
  retains the positioned page and a slab of it at most three viewports tall
  (bounded by the adapter's texture limit); a scroll step inside the slab
  uploads only a one-line status strip and a 32-byte view uniform, and the
  shader masks everything outside the band of complete chord/lyric groups.
  Measured in a debug build with `CPL_TIMING=1`: a slab composes in 50–120 ms,
  a view step in 2 µs for a fitted page and 3–9 ms for a scrolling one (the
  status strip raster). Captured at the top and at `--scroll 120` of the
  scroll sample. Not yet measured: a release build, a key-driven scroll run,
  and the GPU upload cost itself.
- [x] Handle a lyric line wider than the content box without drawing under the
  readability floor. **Decided: wrap.** A line wider than the content box
  breaks after a word that ends in whitespace into continuation rows with a
  `wrap_indent` hanging indent; chords stay over their syllables, a word is
  never split, and a larger body size is taken only if it wraps no more rows
  than the floor does. The width-needed state remains only for an
  unbreakable run, and a scrolling page clips a too-wide title instead of
  withholding the song. Captured at 2x: the scroll sample at 280×420 pt
  (two lines wrap, chords follow their words) and Scarborough Fair at
  420×720 pt (step 9 inline chords, one wrapped line). Under 400 pt the
  scroll status shortens to section, SCROLL and an arrow so it fits.
- [x] Add paged mode for songs that cannot fit and need a predictable page turn.
  Keep whole chord/lyric pairs together and make the current page obvious.
  `v` toggles scroll and paged; Space, ↓ and Page Down turn forward, ↑ and
  Page Up back, Home and End to the ends. Each page begins on the first
  chord/lyric group the page before could not hold whole, so no line repeats
  or is skipped, and the status reads the section and `PAGE n / N`. A page is
  an offset over the same retained slab, so a turn uploads only the status
  strip unless it leaves the slab. `--paged` and `--page N` open in paged
  mode for captures. Captured pages 1, 2 and a clamped 9 of the scroll sample
  at 560×340 pt; a headless test checks every line appears once, in order.
  Not yet: the key-driven turn in a window, and a page-turn mark beyond the
  status strip if rehearsal finds the strip too quiet.
- [ ] Check actual CoreText extents at 1x and 2x against fit bounds, including
  title, byline, labels, and long chords. `tests/page_bounds.rs` now checks
  real CoreText ink at 1x and 2x for the sample, both testdata songs at five
  window sizes (roomy, stand, wrapping, narrow, scrolling, plus the scroll
  end), and wrapped lines with long chords. Expanding it found and fixed
  three defects: a title wider than the content box crossed the margin (it
  now wraps by words); a page that needed side labels at the floor kept them
  at a larger size where they no longer fit the margin (such a candidate is
  now unfit); and measuring while another thread rasterised returned wrong
  advances (the shim is now serialised; `tests/coretext_threads.rs`). 1x
  pages were inspected through `cpl render --scale 1`, which runs the worker's
  real page path and the shader's placement rule. **Still to do: a real 1x
  window**, which needs a 1x display.
- [ ] Add a visual regression path for `--view` sample pages at known logical
  sizes. Verify light and dark polarity and the 17–28 pt type range on macOS.
  `cpl render <song|sample> --out page.png --size WxH --scale N [--dark]
  [--scroll P | --page N]` now writes exactly what the window would present,
  headless; what remains is the set of goldens and the comparison.

## P2 — complete the proposed viewer

- [ ] Finish the missing reduction steps for chord diagrams and metadata pills.
- [ ] Reintroduce tablature only after a compact, readable layout is designed
  and tested with long real tabs. Its visibility should be a deliberate viewer
  choice; parser retention stays independent of that choice.
- [ ] Build the library, search, sidecar metadata, and setlist navigation.
- [ ] Run ChordPro 6 conformance against the reference implementation and
  generate the directive status table from the results.
- [ ] Add the iOS host after the app and renderer boundaries are stable.
- [ ] Build the CoreText shim's fonts with `CTFont` rather than AppKit's
  `NSFont`, so measurement is thread-safe without the process-wide lock in
  `cpl-render/src/text.rs`. Needed before a second worker or the iOS host.
