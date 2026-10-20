# History

This is a record of implemented work and observed checks. `TASKS.md` holds
open work; `PROPOSAL.md` holds design decisions. Dates are local to the project
session. A passing headless test does not stand in for a visual window check.

## 2026-10-21

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
