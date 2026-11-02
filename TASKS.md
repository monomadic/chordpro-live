# Tasks

Ordered by impact on a singer using the macOS viewer. `PROPOSAL.md` holds the
product decisions; this file tracks the work needed to make the current build
meet them. A task is complete only after a real window check and the relevant
automated checks. When every task in an epic is checked, the epic moves to
`HISTORY.md` (see AGENTS.md, "Working from the task list").

## P1 — dependable performance behavior

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
  pages were inspected through `chordpro-live render --scale 1`, which runs the worker's
  real page path and the shader's placement rule. **Still to do: a real 1x
  window**, which needs a 1x display.

## P2 — complete the proposed viewer

- [ ] Check tablature against real scraped tabs once the corpus has some:
  irregular staves, missing bars, tabs mixed with lyrics, and legends.
- [ ] Build the library, search, sidecar metadata, and setlist navigation.
  Split into:
  - [ ] Saved setlists in the sidecar: an ordered list of fingerprints,
    named, opened from search. Needs a design first: how a setlist is
    built, named, and chosen (no Figma frame exists for it yet).
- [ ] Parse chords inside a comment (`{comment: [C]Capo}`), as the
  reference does; `chordpro-live conform` reports it. Needs a design for drawing a
  comment's chords.
- [ ] Draw the delegated notation the parser keeps (`abc`, `ly`, `svg`):
  SVG first, since it needs no external program; abc and Lilypond need a
  renderer, which is a dependency question for the user.
- [ ] Make `%{key_actual}` (and any substitution of a value the viewer
  changes) follow the live transpose; substitution now happens at parse.
- [ ] Honour markup, `{chord}`, `{image}` and the layout hints on the
  page; they agree with the reference on parse only.
- [ ] Finish the bundle as PROPOSAL.md's packaging section describes:
  `CFBundleDocumentTypes` for the song extensions together with
  `open_shim.m` (Finder delivers opened files as an Apple Event, not
  argv), `check-icon.swift`, and the version baked into the binary.
- [ ] Add the iOS host after the app and renderer boundaries are stable.
