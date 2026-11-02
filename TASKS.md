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
  - [x] Opening a song from search makes the visible results the setlist,
    so `n`/`p` step through what was filtered, in its order. Tested
    headless (`Worker::search_input` with Enter) and in 2x windows through
    `--search Q --open N [--next K]`, which sends the worker the keys a
    performer would press (Home, Down, Enter) and then steps as `n`/`p`
    do: rows 2 then 3 and 1 of the unfiltered library opened in order, and
    `--search "+folk " --open 1 --next 5` stopped on the filter's last song.
    Real keypresses in a window are still unexercised.
  - [x] Last-opened in the sidecar and a live rescan. The worker notes
    `opened` (a UTC date-time) once per song change, after the page has
    gone out, and search's Tab sort gains RECENT. While search is open the
    library folders are rescanned every `search_rescan_seconds` (2), keeping
    the selection on the same file. Checked in a 2x window: a song saved
    into the folder appeared and a deleted one went within one interval;
    an `--open 2 --next 2` run wrote both shown songs' `opened`. The RECENT
    sort is covered headless only; no flag reaches it.
  - [ ] Saved setlists in the sidecar: an ordered list of fingerprints,
    named, opened from search. Needs a design first: how a setlist is
    built, named, and chosen (no Figma frame exists for it yet).
  - [x] Transpose-at-open: remember each song's transpose in the sidecar
    and restore it when the song opens. `transpose = n` in the song's
    table, written whenever the live transpose changes and dropped at 0;
    a setlist made at launch or from search opens each song at it. Tested
    headless (worker writes, a fresh worker and a search open restore,
    reset forgets) and in 2x windows: a `--transpose -2` run wrote the
    sidecar and a second launch with no flag opened Greensleeves in Gm.
- [x] Live transpose (`[` `]`, `0` resets) with chordpro-tui's spelling
  rules and `{transpose}` honoured, chord diagrams following it, and the
  status strip showing the offset (PROPOSAL.md, "The app"). Ported
  `transpose.go` and its tests into `cpl_song::transpose`; `{transpose: n}`
  (with `f`/`s` and empty to cancel) shifts the chords after it and sets
  `key_actual`, which the KEY pill shows. The worker's `Setlist` keeps a
  transpose per song, within an octave, and re-measures only that song;
  the old page stays up until the new one is composed. The status strip
  names the key after the mode as the mock does (`FIT · Bm +2 · …`) and
  the fit hints lead with `[ ] transpose`, replacing `n p songs`.
  `--transpose N` opens transposed (`chordpro-live render` too). Checked in a 2x
  window (Greensleeves at +2: chords, folded tabs and status) and headless
  at 1x; a new golden holds House of the Rising Sun at −3, and the status
  change re-blessed seven others. Not yet: real `[` `]` `0` keypresses in a
  window, and the mock's `c chords` hint, which waits on a chord viewer.
- [x] Match the page body to the Figma mock as the header now is. Chords
  are `Page/Chord` SF Pro Semibold 15 pt and labels `Page/Label` Bold 14 pt
  tracked 8%, both ink-soft (a chorus label is ink); a chord/lyric pair
  repeats every 53 pt; sections are 18 pt apart; columns 32 pt; the status
  strip is SF Pro 12 pt ink-soft with the mode's keys flush right; the inks
  are the mock's Polarity variables (ink-faint 184/74, ink-wash 242/22).
  Checked against the mock at 1x through `chordpro-live render` (label, chord, lyric
  and pitch offsets exact) and in light and dark 2x windows; goldens
  re-blessed. Not matched: `Page/Comment` is italic in the mock and upright
  here (the shim has no italic face), and the mock's status keys name
  transpose and a chord toggle that do not exist yet.
- [x] Run ChordPro 6 conformance against the reference implementation and
  generate the directive status table from the results. `chordpro-live conform`
  runs the reference (ChordPro 6.101.0 from CPAN, in `~/perl5`) through
  `scripts/conform/ChordPro/Output/CplJSON.pm`, an output backend that
  writes its parse as JSON (the reference has no JSON output of its own),
  and compares songs, metadata, structure, chords and text with
  `cpl-song`'s. The table in the song note comes from 53 probe songs and
  six files (testdata and the reference's two examples); `--check` and
  `tests/conform.rs` fail when it is stale. 44 of 53 constructs and 5 of 6
  files parse alike. It found one parser defect, fixed here: an unlabelled
  tab was given the label "Tab"; the label is now kept as written and the
  page still says TAB. Not a window change; goldens unchanged.
- [x] Close the parse differences `chordpro-live conform` reports, each against the
  spec and then the table: `{start_of_grid}`, the delegated `abc`, `ly`,
  `svg` and `textblock` environments (Molly Malone opens with `abc`),
  `{new_song}`, `[*annotation]` (kept as a chord named `*…`), `%{…}`
  substitution, and selectors such as `{comment-guitar}`. Each was probed
  in the reference first and matched to what it printed. Grids are cells
  (margin, bars, chords, comment) drawn as aligned monospace rows with
  their chords transposed; text blocks read as verses; both use the
  compact inline rows. abc, ly and svg are kept verbatim, diagnosed and
  left off the page. Annotations sit in the chord row and never
  transpose. Selectors match the guitar or a set metadata item, and a
  skipped environment drops to its end. `%{…}` follows the reference
  case by case. A file of several songs is several setlist songs and
  search rows, each with its own fingerprint; a file of one song keeps
  the whole file's. Six new probes cover margins, conditionals and
  selector forms, and `testdata/harbour_lights.cho` uses all of it: 59 of
  60 constructs and 7 of 7 files parse alike. Window checks at 760x900:
  both songs of that file, the second transposed +2, search listing both
  and opening the second. New golden `harbour-transposed-760x900@1x`; the
  two search goldens re-blessed for the larger library.
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
- [x] Give the `.app` a library folder when Finder launches it: a bare
  launch opens the library of the current folder, which for a Finder
  launch is `/`. A bare launch now opens the config's new `[library]
  folders`, else the current folder, else (started in `/`) `[library]
  app_folder`, `~/Music/ChordPro` by default, made on first launch. A unit
  test covers each case and the table's errors. Checked by launching the
  built `.app` through LaunchServices (`open`): it ran in `/` and opened an
  empty library at `~/Music/ChordPro` at once. `scripts/window-id.swift`
  now also finds the bundle's windows, which macOS names "ChordPro Live".
- [ ] Finish the bundle as PROPOSAL.md's packaging section describes:
  `CFBundleDocumentTypes` for the song extensions together with
  `open_shim.m` (Finder delivers opened files as an Apple Event, not
  argv), `check-icon.swift`, and the version baked into the binary.
- [ ] Add the iOS host after the app and renderer boundaries are stable.
- [x] Build the CoreText shim's fonts with `CTFont` rather than AppKit's
  `NSFont`, so measurement is thread-safe without the process-wide lock in
  `cpl-render/src/text.rs`. Needed before a second worker or the iOS host.
  The shim now uses CoreText, CoreGraphics and CoreFoundation only (no
  AppKit link in `cpl-render`), and the lock is gone. Faces are the system
  UI font with a weight trait, plus the monospace trait after the weight;
  every weight was checked against AppKit's face by name, and all
  seventeen goldens are pixel-identical. The first attempt applied the
  weight after the monospace trait, which CoreText ignores, and the
  goldens caught the regular-weight diagram names; a unit test now pins
  that. `tests/coretext_threads.rs` passed five runs in a row without the
  lock. The shim's unused truncation, tabular-figure and caret-offset
  options went with it. Window-checked at 2x.
