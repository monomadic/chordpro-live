# Tasks

Ordered by impact on a singer using the macOS viewer. `PROPOSAL.md` holds the
product decisions; this file tracks the work needed to make the current build
meet them. A task is complete only after a real window check and the relevant
automated checks. When every task in an epic is checked, the epic moves to
`HISTORY.md` (see AGENTS.md, "Working from the task list").

## P1 — dependable performance behavior

- [ ] Check actual CoreText extents at 1x and 2x against fit bounds in a
  **real 1x window**, which needs a 1x display. Everything else is done
  (HISTORY.md, 2026-10-21): `tests/page_bounds.rs` checks real CoreText ink
  at 1x and 2x across the testdata songs and window sizes, and 1x pages
  were inspected through `chordpro-live render --scale 1`.

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
