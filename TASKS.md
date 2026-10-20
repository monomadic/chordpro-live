# Tasks

Ordered by impact on a singer using the macOS viewer. `PROPOSAL.md` holds the
product decisions; this file tracks the work needed to make the current build
meet them. A task is complete only after a real window check and the relevant
automated checks.

## P0 — make the song page usable

- [x] Temporarily hide tablature blocks in the viewer. Keep them in the parsed
  `Song` and the `.cho` file. Do not let a hidden tab consume fit or scroll
  space. Headless tests and the real-song macOS window check passed.
- [x] Reproduce the supplied **“What's Up”** screenshot with the source song
  and a fixed `--view` window size. Capture fit and scroll states; check title,
  intro, verse, chorus, chord alignment, margins, and the bottom edge. Keep a
  small, redistributable regression fixture or equivalent test case. Use
  `--view <song> --size 980x720` for the 1960×1504 px Retina window. The
  initial and Page Down positions were inspected; focused synthetic tests
  cover hidden tab, chord-only Intro, section context and post-tab plain text.
- [x] Make the first viewport show a coherent part of the song. The screenshot
  shows an isolated chord row near the bottom and a large gap above it. Audit
  scroll group bounds, section breaks, and clipping so a chord row never
  appears without its lyric line and no visible line is needlessly withheld.
  The real-song window now shows the Intro, the complete first verse lines,
  and no orphaned chord row; Page Down also preserves pairs.
- [x] Check chord-only lines and section labels in real songs. An `Intro` with
  only chords should read as intentional musical structure, not as a lyric
  line with missing words. Compact chord-only rows and headless tests landed;
  the supplied song shows a compact Intro chord row and Verse 1 label. After
  Page Down, the status says CHORUS while its label is off screen.
- [x] Verify the title displayed in the screenshot. Determine whether the
  quotation marks are authored in the ChordPro title or introduced by parsing;
  only change parsing if a minimal fixture proves the latter. The source
  explicitly contains `{title: "What's Up"}`.
- [x] Decide how the viewer should handle notation legends immediately after
  hidden tab blocks. The supplied song has a technique key after `{end_of_tab}`
  as ordinary text, and it currently appears as a large lyric line. Keep text
  outside a tab block visible; it may be a performance instruction. Authors
  can put tab-only notation inside the block to hide it with the tab. The app
  never changes the source file.

## P1 — dependable performance behavior

- [ ] Move song measurement and page preparation off the window callback.
  Preload the next song, and swap only complete pages so opening a song or
  resizing cannot stall or visibly reflow during performance.
- [ ] Replace full page recomposition and texture upload on every scroll step
  with retained drawing or a moving viewport over a prepared page. Keep the
  existing text coverage cache, bound its memory, and measure scroll latency.
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
