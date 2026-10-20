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
- [ ] Handle a lyric line wider than the content box without drawing under the
  readability floor. Decide and implement horizontal paging, wrapping, or an
  explicit width-needed state with a useful recovery path.
- [ ] Add paged mode for songs that cannot fit and need a predictable page turn.
  Keep whole chord/lyric pairs together and make the current page obvious.
- [ ] Check actual CoreText extents at 1x and 2x against fit bounds, including
  title, byline, labels, and long chords. A real-raster page-bound test now
  covers three viewport sizes at both scales with no mismatch; expand it when
  a failing real-song case appears and verify a 1x window visually.
- [ ] Add a visual regression path for `--view` sample pages at known logical
  sizes. Verify light and dark polarity and the 17–28 pt type range on macOS.

## P2 — complete the proposed viewer

- [ ] Finish the missing reduction steps for chord diagrams and metadata pills.
- [ ] Reintroduce tablature only after a compact, readable layout is designed
  and tested with long real tabs. Its visibility should be a deliberate viewer
  choice; parser retention stays independent of that choice.
- [ ] Build the library, search, sidecar metadata, and setlist navigation.
- [ ] Run ChordPro 6 conformance against the reference implementation and
  generate the directive status table from the results.
- [ ] Add the iOS host after the app and renderer boundaries are stable.
