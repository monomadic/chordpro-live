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
  were inspected through `chordpro-live render --scale 1`. The repeatable
  capture matrix and the review procedure for that window check are in
  [docs/visual-regression.md](docs/visual-regression.md), with the last
  attempt recorded in
  [docs/visual-regression-validation.md](docs/visual-regression-validation.md);
  a candidate capture alone does not complete this task, and the 1x and 2x
  runs still need reviewing and approving as baselines.

## P2 — complete the proposed viewer

- [ ] Build the library, search, sidecar metadata, and setlist navigation.
  Split into:
  - [ ] Saved setlists in the sidecar: an ordered list of fingerprints,
    named, opened from search. Needs a design first: how a setlist is
    built, named, and chosen (no Figma frame exists for it yet).
- [ ] Parse chords inside a comment (`{comment: [C]Capo}`), as the
  reference does; `chordpro-live conform` reports it. Needs a design for drawing a
  comment's chords.
- [ ] Keep a section the spec does not name as the reference does:
  `{start_of_tablature}` (in the iCloud tab library) opens a section
  named `tablature` there, an unnamed stanza here. `conform` reports it
  per file; add it to the probe constructs.
- [ ] Lenient import of tab outside a tab section: about a dozen songs in
  the iCloud tab library (Riptide, How Soon Is Now, True Colors) have
  staff lines in a verse, drawn as lyrics. PROPOSAL.md's lenient import
  should recognise them and draw them as tablature.
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
