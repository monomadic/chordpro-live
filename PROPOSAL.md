# chordpro-live — technical proposal

*Status: implementation started, 2026-10-20. A partial parser and a
multi-column macOS viewer exist; all nine reduction steps are built, then
scrolling or paging. Tablature draws as small, soft monospaced staves that
wrap into systems at bar lines (see the ladder's step 5), and the reader can
pin it shown or folded. The library, conformance work, and iOS host remain
proposed. Where a decision is settled it says so; where
it is an unknown it says what will settle it.*

## The pitch in one paragraph

A GPU-rendered ChordPro **viewer** for the stage, on macOS and iOS. One screen
holds the whole song: chords over lyrics, flowed into balanced columns, set in
black on white or white on black like a clean manuscript page. When a song
cannot fit at a readable size the app does not squeeze; it compacts by throwing
away the least important elements in a fixed order, and when that is not enough
it scrolls or pages. Lyrics outrank chords. A singer should be able to follow
the words the way they follow a karaoke line. Around the page sit a library
search with tags and ratings, and a chord viewer. It is not an editor and never
writes a `.cho` file.

It is the third sibling of three local projects and borrows from each:

| From | Takes |
|---|---|
| **chordpro-tui** (Go) | The product idea and its proof, and one lesson in the negative: it never had a format standard it could claim to meet, so this project starts from the ChordPro 6 spec (below). The parse → `Song` → render pipeline, the fit algorithm (spacing plans, balanced column packing, fewest columns that fit), the **reduction ladder** of tri-state `auto` options, side section labels, the chord database and `{define}` override, transpose spelling rules, the tempo-driven and duration-synced scroll modes, the fuzzy finder whose metadata columns drop narrowest first, and the `chordpro-pdf` exporter's **em-unit layout** (measure once at scale 1, then scale = min(availW/w, availH/h), picking the column count that maximises it). |
| **switchblade** (Rust) | The host architecture. The `App` trait boundary (no winit/wgpu types in the app crate; input in as normalised events, a plain `Frame` out), the pure `schedule.rs` redraw policy (100 ms idle tick, `animating` means visible motion only, occlusion never runs the hot loop, 4 ms runaway floor), the coalescing `Waker`, **CoreText whole-string rasterisation** into an R8 atlas with grayscale AA, the raster budget that rotates, quantised cache keys, byte-range **span highlights** drawn from the same atlas entry, the non-sRGB surface, TOML config and theme files with hot reload, the "render thread never touches the filesystem" rule, the derived version number, and the Xcode-free `.app` recipe. Its iOS proposal (`switchblade/ios/PROPOSAL.md`) is the template for the iOS section below. |
| **abner** (Rust) | How to stay slim. One default TOML that is the complete key set, overlaid and then checked with `deny_unknown_fields` plus a test that the default is complete and valid. One instanced-quad pipeline with an SDF rounded box and `fwidth` AA. "Never estimate what the renderer can measure." UI colours authored as sRGB hex and decoded once. A `--view` flag for every state so screenshots never need injected keystrokes. The warning that ab_glyph over hardcoded system font paths renders **no text at all** when the paths move. Titlebar glass and `setHasShadow(false)` for a chromeless window. |

## Product contract

Apply these in order when trade-offs conflict:

1. **The page is always readable.** Body text never drops below the readability
   floor. Fitting is a goal; readability is a constraint.
2. **Nothing surprising happens during a song.** No layout reflow, no atlas
   blink, no I/O stall, no screen sleep, no animation that was not asked for.
3. **Lyrics first.** When space is short, chords, labels, metadata and diagrams
   give way before a single lyric does.
4. **One screen if it can be, scroll if it must.** Fit mode is the premise;
   scroll and paged modes are the fallback, chosen by the app, never silently.
5. **Instant song change.** The next song in the folder or setlist is laid out
   before it is asked for.
6. Simplicity and feature completeness.

Rule 2 is switchblade's "attention preserving interaction" with a performer in
place of a viewer. A correct page that reflows mid-verse is not a complete
change.

## Architecture

```
chordpro-live/
  Cargo.toml                 workspace
  src/main.rs                the macOS binary: winit loop + AppKit shims (thin)
  crates/cpl-song            parser, Song model, transpose, chord database      (pure Rust, no OS)
  crates/cpl-layout          measurement, fit search, reduction ladder, Page    (pure Rust, Measure trait)
  crates/cpl-app             implements App: views, library, tags, input map   (headless, no winit/wgpu)
  crates/cpl-render          wgpu renderer: quads + text atlas, TextRaster      (knows wgpu, not winit)
  crates/cpl-ios             iOS host (see "iOS")
  packaging/                 build-app.sh, Info.plist.in, version.sh, check-icon.swift
  ios/                       build-ios.sh, Info.plist.in, entitlements
  testdata/                  songs of every length, golden renders
  docs/architecture/         one note per subsystem, written as each lands
```

Data flows one way, as in chordpro-tui: **parse → Song → measure → fit → Page →
Frame → GPU**. `cpl-song` and `cpl-layout` are pure and testable on any
machine. `cpl-app` is switchblade's `sb-app`: it knows the product and nothing
about the OS. `cpl-render` is what switchblade's iOS proposal calls
`sb-render`, extracted from day one rather than later: a renderer that takes a
`wgpu::Surface` and a `Frame` and does not know which OS made the surface. The
boundary is not hygiene. It is the thing that makes the iOS host a second
caller of the same code instead of a port.

### The song model (`cpl-song`): ChordPro 6, in full

chordpro-tui grew its parser directive by directive and never had a standard
it could claim to meet. This project starts the other way round: **the parser
is written against the current ChordPro specification (version 6, the
reference implementation at chordpro.org) and claims conformance to it**, and
chordpro-tui's parser tests and `testdata/` are kept only as a regression
floor. The working specs in `~/src/chordpro-library` are the local authority;
the per-directive pages at chordpro.org are the final one. The parser gets
its own `docs/architecture/chordpro.md` listing every construct in the spec
with its status, and that table is kept exact.

The model is the TUI's shape, widened: `Song` → `Section` (kind, label) →
`Line` → `Segment` (a chord *or* an annotation plus the lyric text it sits
over), a generic metadata multimap, a `{define}` map, `TransposeBy`. What
"in full" means, by area:

| Spec area | Status in v1 |
|---|---|
| **Directives** — long and short names, `:` and whitespace argument separators, HTML-style quoted attributes (`{start_of_verse label="Verse 1"}`), `x_*` private directives (kept, ignored) | Parsed |
| **Conditional selectors** — `{define-ukulele: …}`, `{comment-tenor: …}`, `!` negation, selector on the start of a pair only | Parsed and evaluated against a configured instrument and user; a selector is the only thing that makes the same file render differently for two band members |
| **Metadata** — every standard item (`title subtitle artist composer lyricist arranger copyright album year key time tempo duration capo sorttitle tag`), repeated items, `{meta: name value}` as the generic form, read-only `_key` after transpose | Parsed; `tag` metadata is a file-authored tag in the library |
| **`%{…}` substitution** — `%{name}`, `%{name|true|false}`, `%{name=value|…}`, `%{}` inside a conditional | Evaluated in every text that the spec allows it in |
| **Lyric lines** — `[chord]`, `[*annotation]`, chords mid-word, chord-only lines, lines of only chords, `\t`, trailing-space rules | Parsed |
| **Markup** — `<b>`, `<i>`, `<u>`, `<span …>` and the other Pango-style tags the spec admits in lyrics and comments | Parsed to styled runs; the renderer draws weight and italic, and ignores colour (the page is black and white by design) |
| **Chord names** — a real chord grammar (root, quality, extensions, bass) in the Common, Dutch, German, Latin and Scandinavian note-name systems, Nashville and Roman numbers, note-only mode | Parsed and transposed; the fixed accidental spelling rule stays the *default*, `{transpose: 2f}`'s `s`/`f` suffixes override it |
| **`{key}`, `{capo}`, `{transpose: n}`** and their interaction (transpose applies from that point on, creates `key_actual`) | Honoured; a mid-song `{transpose}` is a per-section offset in the model |
| **`{define}`** for string instruments (`base-fret`, `frets`, `fingers`, keys), for keyboards (`keys`), `{chord}` to show a diagram inline, and the `copy`/`display`/`diagram` modifiers | Parsed; string and keyboard diagrams both render in the chord viewer |
| **Environments** — verse, chorus, bridge, tab, grid, with labels; `{chorus}` and `{chorus: Label}` recall | Parsed; a recalled chorus is a reference in the model, which is what makes ladder step 8 (collapse repeated choruses) spec-native rather than a heuristic |
| **`tab`** — verbatim, whitespace significant, `[` not a chord | Rendered monospaced, verbatim within each system; a staff wider than the page breaks into systems at a shared bar line (or between frets), repeating the string names |
| **`grid`** — cells, `.`, `/`, `~`, every bar symbol and volta, `shape=` and margins | Parsed and rendered as a chord grid (new; the TUI never had it) |
| **Layout hints** — `{new_page}`, `{column_break}`, `{columns: n}` | Parsed and honoured as hints by the fit engine: a `{columns}` pins the column count when it fits the floor, a break is a hard block boundary |
| **Formatting directives** — the `*font`, `*size`, `*colour` families, `{titles}`, `{diagrams}` | Parsed, **deliberately not applied**. The spec says the format defines the source, not the appearance; this viewer owns the appearance. They are listed in the conformance note as "accepted, ignored", never as unsupported |
| **`{image}`** | Parsed; rendered in v1 as a placeholder box with its `src`, drawn for real once the quad pipeline has an image slot (a small, known piece of work) |
| **Delegated environments** — `abc`, `ly`, `svg`, `textblock` | Parsed and preserved; `textblock` renders as text, the other three are listed under unknowns |
| **Encoding and hygiene** — UTF-8 with BOM, CRLF, smart quotes and dashes left as authored, blank-line rules, comments (`#` lines) | Handled |

**Conformance is measured, not asserted.** The reference implementation is a
Perl program that installs with one `cpan`/`brew` command, and it has a
`--generate=JSON` (or equivalent structured) output. The test suite runs it
over the corpus and over the spec's own examples, and compares the parsed
metadata, section structure, chord list and lyric text against `cpl-song`'s.
A divergence is a bug in this parser until shown otherwise. This is the
footing the TUI never had.

The chord database starts from the same `guitar.json`, embedded, and gains a
ukulele set and the keyboard voicings the spec's `{define}` keyboard form
implies.

Two additions the format does not specify:

- **Lyric syllables.** A `Segment` already breaks a line at every chord. For
  karaoke the layout also needs word boundaries inside a segment, so the model
  exposes `words()` over a line with byte ranges back into the source text.
  Ranges, not copies: the text stack highlights by byte range (below).
- **Lenient import.** The user's `tab-scraper` corpus will contain plain
  chords-over-lyrics text and loose ChordPro dialects. The parser gets a
  detection pass that recognises a chord-only line above a lyric line and
  folds it into segments. Detection is a library-ingest concern, so a file
  that fails it is listed with a warning badge rather than refused.

### Layout (`cpl-layout`)

This is the heart of the product and the biggest departure from the TUI,
which worked in character cells. The PDF exporter already solved the
continuous version, and its scheme is adopted whole:

**Every dimension is in em units**, multiples of the body size. Line heights,
gutters, insets, chord gap, label size, diagram geometry are constants in em
(the exporter's `layout.go` table is the starting set). A song is measured
**at the floor and at each whole-point candidate up to the configured maximum**
into blocks of lines of runs, each
run with an x offset and a width in em, chords pushed right of their syllable where
they overhang exactly as the TUI does. Measurement is the only part that needs
the OS, through a one-method `Measure` trait. Font advances scale nearly
linearly with size under CoreText, but are not exactly linear for the system font, whose tracking and
optical size change with point size (about 5% narrower at 34 pt than twice
17 pt). Candidate measurement at the actual drawn size prevents that difference
from accepting a page that overflows. The initial implementation does this
synchronously on song load; worker preparation remains future work.

**The fit search is then arithmetic.** The ladder is chosen at the readability
floor. Within that ladder step, the largest measured candidate that fits is
drawn. For each column count 1..=N,
`pack_into` balances the blocks (chordpro-tui's greedy fill with a rising
target, floats instead of rows), and the scale that fills the viewport is
`min(avail_w / layout_w, avail_h / (header_h + gap + max_col_h))`. The count
that maximises scale wins. This runs in microseconds and is the reason resize,
rotate and transpose can re-fit on the frame they happen.

**The readability floor is where this project differs from both parents.**
The PDF exporter had only a *maximum* body size, because a PDF is zoomable.
The TUI had cells, which cannot shrink. Here the body size has a floor in
**points** (logical, so it is the same physical size on a 1x and 2x screen),
with presets for viewing distance:

| Preset | Body floor | Meant for |
|---|---|---|
| `handheld` | 13 pt | phone in hand, tablet on a lap |
| `stand` | 17 pt | tablet on a music stand, laptop on a desk |
| `far` | 22 pt | monitor across a room |

The floor is a tunable, not a constant, and the numbers above are to be
measured on a real stand and not believed. **Settled (2026-11-01): the
reader sets it.** `body_size` in a user `config.toml` (Application
Support, `$CPL_CONFIG` or `--config`) or `--floor PT` sets the floor in
points, 6 to 72; a reader at a laptop may take it to 9 or 10 pt. There is
no DPI arithmetic: the reader picks the size that reads at their
distance, and the default stays 17 pt. When the best fit lands below the
floor the layout engine does **not** draw it. It descends the ladder.

**The reduction ladder** is chordpro-tui's, extended for the page. Each step
is applied, the fit re-run, and the first layout at or above the floor wins.
Least destructive first:

1. Section spacing gives back its blank rows.
2. Section labels move to the side margin (if the margin does not cost a column).
3. *(removed 2026-10-29)* The header no longer collapses to one line. The
   step applied only when the window was wide enough, so maximising a window
   could collapse the header and drop the text to the floor while a smaller
   window kept both. The header always keeps title and byline on their own
   lines; the other steps keep their numbers.
4. Chord diagrams leave the header.
5. Tablature sections fold to their label, which says `FOLDED`.
   **Settled (2026-10-29): tablature is a part for the player, not the
   singer's text.** It is set in the monospaced face at `tab_size` (0.78 of
   the body) in the soft ink, beside a faint rule, so it never outweighs the
   words; its first rendering at body size and strong ink overwhelmed them.
   A staff (consecutive tab lines) is one unit: when it is wider than the
   page it breaks every row at the same column, preferring a bar line,
   otherwise between frets, never inside a fret number or technique, as
   evenly as the fewest systems allow, and each continuation system repeats
   the string names. A system is one scroll and page group. The reader's
   `t` pins tabs shown or folded and takes this step out of the ladder.
6. Metadata pills drop.
7. The title drops.
8. *(new)* Repeated choruses after the first collapse to their label and first
   line. The singer knows the words; the chords were shown once. A `{chorus}`
   recall is already this in the model; a chorus the author retyped in full is
   detected by text equality.
9. Chords go inline with the lyric, superscript. This halves line height at
   the cost of chord-over-syllable alignment, and it is the last step that
   keeps every lyric on screen.
10. **Fit fails.** The app switches to a scrolling or paged mode and says so in
    the status strip. It never draws a page under the floor.

Each step is a tri-state option (`on`, `off`, `auto`) exactly as the TUI's
config, so a user who always wants inline chords, or never wants choruses
collapsed, pins the step. Step 10 is the behaviour the brief asks for in
"isn't trying in vain to squeeze everything into view".

**Lyrics first** is enforced by the ladder's order and by one more rule inside
a line: when a lyric run and a chord run compete for width, the chord row
shrinks to its own floor (0.8 of body) before the lyric does, and chord
overhang pushes later lyrics right rather than compressing them.
Chord-only progressions, such as an intro, use one compact row at the normal
chord size; they do not reserve an empty lyric row or shrink at the inline
chord step.

The output of fit is a **`Page`**: a list of positioned items in points (text
runs with face, size, byte range and role; hairlines; diagram primitives) plus
the chosen ladder level, column count and whether the floor was met. `Page` is
pure data. The scroll views window over a `Page` laid out as one tall column
with no height constraint, which is chordpro-tui's `RenderLong`.

**Cost control.** Layout is never computed on the frame path from scratch.
`cpl-app` holds a `LayoutCache` keyed by `(song fingerprint, transpose,
viewport in points, options)`, computes on change, and pre-computes the next
and previous song in the queue on a worker. The measurement step calls CoreText
synchronously, so it too runs on the worker; the frame path only reads a
finished `Page`.

### Text and vector rendering (`cpl-render`)

The brief asks for a manuscript page: black and white, crisp, "perfectly
aliased beautiful fonts and vector rendering". That sets the rendering rules.

**Text is rasterised by CoreText, whole strings, into an R8 atlas.** This is
switchblade's `text_shim.m` and `TextRaster` trait, brought across unchanged
in design: `kCGImageAlphaOnly` for one coverage byte per pixel, subpixel AA
off (LCD fringes are wrong the moment the page inverts), rasterised at device
pixels and drawn at logical size, quads snapped to whole device pixels. It
gives real shaping, font fallback and tabular figures for free, and it is the
same framework on iOS. Zero font crates, zero bundled fonts.

Every lyric line, chord row, label and comment is one string, so a song is a
few hundred cache entries at one size. The entries change only when the
layout changes, never per frame, which is the same trade switchblade made and
for the same reason. Three switchblade rules carry over as invariants:

- Keys are `(text, face, size quantised to ¼ px, limit)`; a label that is
  short by construction sets no limit so its key never moves.
- A frame rasterises at most a budget of misses, rotating, so a new page fills
  in over a few frames rather than stalling one; and a page is pre-rasterised
  on the worker while it is still the *next* song, so in practice it arrives
  complete.
- A raster wider than the atlas is refused, never reset for.

**Highlighting is by byte range from the same atlas entry.** Switchblade's
`spans` mechanism (a clipped redraw of the same bitmap in another colour, the
base string cut around the slice so every pixel is painted once) is exactly
the karaoke primitive: the current word in a lyric line is a span, the sweep
across it is the span's end offset advancing. No second raster, nothing to
misregister, no per-frame atlas work. The x offset per UTF-16 boundary that
the shim returns is what positions the sweep and the margin marker.

**Everything that is not text is a quad.** Hairlines, the chorus bar, column
rules, metadata pill outlines, the tab panel frame, the chord diagram grid,
fret dots and open/muted marks are instanced quads through abner's SDF rounded
box with `fwidth` AA. A circle is a rounded box with radius ≥ half its side. A
hairline is a quad exactly one **device** pixel thick, positioned on a device
pixel boundary, so it is a true line and not a gray smear. This is "vector
rendering" in the sense that matters: resolution-independent geometry,
analytically antialiased, with no bitmap scaled anywhere. If a later feature
needs a true path (a slur, a brace), `resvg`/`tiny-skia` rasterises it to the
same atlas, as abner already does for its logo.

**Two colours only.** The theme is a polarity plus an ink ramp: `ink`,
`ink_soft` (chords, labels, metadata) and `ink_faint` (rules, inactive lines),
authored as gray levels and inverted as a whole. The surface is non-sRGB as in
switchblade; with only grays in play the one gamma question left is edge
coverage, and it is listed under unknowns.

**One text pipeline, one quad pipeline**, drawn content-then-overlay as
switchblade does so a panel can sit over the page's own labels.

### The app (`cpl-app`)

Four surfaces, one modal ladder, every key remappable through the same
`[keys]` table switchblade uses, with groups per surface.

**Page** — the song. Fit mode by default; `v` cycles fit → scroll → sync →
paged. Transpose `[`/`]`, reset `0`, polarity `i`, fold tabs `T`, hide header
`h`. The status strip (one line, bottom, `ink_faint`) shows mode, key with
transpose offset, ladder level when any step fired, the section containing
the top visible line in scroll mode, and a `MORE ↓` flag when
a scrolling mode has content below. In scroll mode `+`/`-` nudge the
tempo-derived speed; in sync mode they adjust the `{duration}` and `space`
plays and pauses the progress line, both exactly as chordpro-tui.

**Search** — the library. `o` or `/`. A fuzzy match over title, artist and
(opt-in with `l:`) lyric text; rows in labelled columns — title, artist,
key, capo, tempo, year, rating, tags — that drop narrowest first as the window
narrows, chordpro-tui's finder rule. Filter pills for tag and minimum rating,
in the shape of switchblade's filter bar. **Settled (2026-10-29): filters are
typed, since every letter belongs to the query.** `+tag␣` adds a tag pill,
`*3` a minimum-rating pill, and Backspace on an empty query removes the
newest. Tab cycles the sort. Enter opens the selected row with every row,
in order, as the setlist, so `n`/`p` on the page step through what was
filtered; this replaces stepping the folder from inside search.

**Chords** — `c`. Fingering diagrams for every chord in the current song,
following the live transpose, `{define}` winning over the database, the
`{tuning}` shown. Diagrams are quads (above), so they are crisp at any size
and need no image assets.

**Tags and ratings** — a small editor over library metadata only, never over
the song file. In search, digits 0–5 rate the selected row (0 clears) and
`#name⏎` toggles a tag. Where this is stored is a settled
decision below.

**Live performance affordances**, each a line of code somewhere and each a
reason someone will or will not use the app on stage:

- The screen never sleeps while a page is up (switchblade's `power.rs`
  activity token on macOS; `isIdleTimerDisabled` on iOS).
- Bluetooth page turners (AirTurn and the like) are HID keyboards sending
  Page Up/Down or arrows. They work through `[keys]` with no pedal code.
- Fullscreen is chromeless: titlebar glass, `FullSizeContentView`,
  `setHasShadow(false)`, abner's recipe.
- Song change is instant because the neighbours are already laid out.
- Nothing animates on the page except the karaoke marker and a scroll.

### Lyrics for singers

The brief asks for lyrics to take precedence and to be more karaoke-like.
Three layers, each useful without the next:

**1. Lyric-first typography (no timing needed).** Lyrics in full `ink` at body
size; chords in `ink_soft`, bold, at 0.8 em so they read as annotation on the
words rather than the other way round. A `lyrics` display option strips the
chord rows entirely for the singer's device while the guitarist's shows them;
the ladder's inline-chord step is the midpoint. Section labels in the side
margin, small caps, so the eye runs down one left edge of text.

**2. Line following (coarse timing).** A margin marker — a short hairline or a
filled dot in the gutter, the kind of thing a copyist pencils in — sits beside
the current line; lines above it fade to `ink_faint`. The current line
advances by:

- the sync clock when `{duration}` is known (chordpro-tui's player mode,
  which already maps elapsed time to a position so the last line lands at
  the end);
- tempo and time signature when `{tempo}` is known, assuming one lyric line
  per bar pair, which is wrong often enough to be a hint and not a mode;
- the **pedal**: a tap advances one line. For most live use this is the mode.

**3. Word sweep (fine timing).** When per-word times exist, the span highlight
sweeps across the current line. Timing sources, none of which edit the song:

- **LRC files.** The karaoke-timing format is everywhere and the match is
  text to text: LRC lines are aligned to lyric lines by fuzzy string match,
  and an LRC with word-level tags gives the sweep for free. A `song.lrc`
  beside `song.cho`, or a lookup in a configured folder.
- **Tap timing.** During rehearsal, holding a key while the song plays
  records one timestamp per word boundary into the library's metadata (not
  the file). Crude, but a singer's own taps are the timing they want.
- Audio following (beat tracking from the microphone) is out of scope and
  listed as a future unknown.

Timings live in the sidecar store keyed by song fingerprint, so a re-tagged
or moved file keeps them.

### Library, tags, ratings

The TUI had a folder. This app has a library: one or more folders (on iOS,
Files-app bookmarks), scanned on a worker into an in-memory index of parsed
metadata, with the parse itself cached by `size_mtime` fingerprint as
switchblade keys its sidecar cache. Nothing on the frame path stats a file.

**Tags and ratings go in a sidecar, not the file.** Settled, because the app
is not an editor and must never rewrite a user's chart. The store is one
TOML file in the config directory (`library.toml`), a table per song keyed by
content fingerprint, holding tags, rating, timings, last-opened, and
transpose-at-last-open. The fingerprint is the SHA-256 of the file's
normalised text, not its path, so a rename or a move between devices keeps
the metadata; a `{meta: tags ...}` or `{tag:}` directive already in a file
is read as a file-authored tag, shown, and never written back. A single file
is chosen over SQLite for the same reason switchblade chose a filesystem
cache: the data is small (a few hundred bytes per song), it diffs and syncs
through iCloud Drive as a plain file, and profiling has not asked for more.

## Design source

**Every surface is mocked in Figma:** [chordpro-live — Screens](https://www.figma.com/design/0ovWyKEx7qyWh38a08Wl82)
(2026-10-18): the Page in both polarities, the lyric-following state, Search and
Chords, all at iPad mini landscape (1133×744 pt) with the `stand` 17 pt body.
The file is built the way the app will be: colours are a `Polarity` variable
collection with Light and Dark modes (`paper`, `ink`, `ink-soft`, `ink-faint`,
`ink-wash`), so the dark page is the light page with the mode switched; type is
the em table above as `Page/*` text styles at body 17; chords sit over their
syllables as nested auto-layout segments, which is the same push-right rule the
layout engine applies; chord diagrams are hairline rectangles and dots, as the
quad pipeline will draw them. SF Pro stands in for the system face and Roboto
Mono for the system monospaced face (JetBrains Mono's ligatures mangle tab
lines, a reminder to disable `liga` in the tab face). Two things the mock
exposed before any code: the song does not fit two columns at 17 pt on that
device and needs three, and whitespace at segment edges must be measured, not
trimmed.

## macOS, Xcode-minimal

Settled, and it is switchblade's and abner's shipped recipe: **no Xcode
project, no Xcode.app dependency.**

- `cargo build --release` produces the binary. AppKit is reached through the
  `objc2-app-kit` crates (window chrome, keep-awake, Open With); the two or
  three Objective-C shims (`text_shim.m`, `open_shim.m`) compile through the
  `cc` crate, which needs only the Command Line Tools' `clang`, which cargo
  needs to link on macOS anyway.
- `packaging/build-app.sh` assembles `ChordProLive.app`: the binary,
  `Info.plist.in` rendered with the version, `AppIcon.icns` made from one
  square unmasked 1024² PNG by `iconutil` (ships with macOS), ad-hoc
  `codesign` (ships with macOS), `lsregister -f` so an in-place reinstall is
  seen. `check-icon.swift` runs under `swift` from the Command Line Tools and
  renders what the OS will composite before a build; abner's notes on the
  macOS 26 icon plate apply.
- The version is derived, never written: `packaging/version.sh` computes
  `major.minor.<commit count>` for both `build.rs` and the plist, so binary
  and bundle cannot disagree.
- `Info.plist.in` declares `CFBundleDocumentTypes` for `.cho .chopro
  .chordpro .crd .pro .cp`, and `open_shim.m` grafts `application:openURLs:`
  onto winit's delegate, because LaunchServices delivers opened files as an
  Apple Event, never argv, and winit 0.30 panics if its delegate is replaced.

Nothing here has a dependency the Command Line Tools do not satisfy.

## iOS, a minimal shim

Apple ships the iOS SDK, the simulator and device tooling only inside
Xcode.app. **Xcode must be installed; it never needs to be opened**, and
there is no `.xcodeproj`. That is the honest version of "Xcode-minimal".

The architecture is switchblade's iOS proposal with one simplification that
project could not make: this app needs no media layer, no subprocess, no
cross-compiled C library. The whole of `cpl-song`, `cpl-layout`, `cpl-app`
and `cpl-render` compiles for `aarch64-apple-ios` as ordinary Rust, wgpu
makes a Metal surface from a `CAMetalLayer`, and CoreText is the same
framework. What iOS needs that the Mac host does not:

| Need | Where it lives |
|---|---|
| A `UIApplication`, a view with a `CAMetalLayer`, a display link, touch and hardware-keyboard events, lifecycle (pause GPU work on resign-active) | the host |
| Folder access through `UIDocumentPickerViewController` and security-scoped bookmarks; enumeration on a host thread; iCloud placeholders surfaced as a pending state | the host |
| `isIdleTimerDisabled` while a page is up | the host |
| Device-class defaults (readability preset `handheld` on iPhone, `stand` on iPad; smaller atlas on iPhone) | `cpl-app` config overlay |

**Three candidate hosts, to be settled by the Phase 0 spike:**

1. **winit's own iOS backend.** winit 0.30 runs on UIKit. If it delivers
   touches, keyboard, resize and lifecycle well enough, the iOS binary is the
   same `main.rs` shape as the Mac's and the shim is the document picker and
   the idle timer, reached through `objc2-ui-kit` from Rust. Smallest
   possible shim; the risk is winit's iOS maturity.
2. **A Rust host on `objc2-ui-kit`.** Declare the view controller and app
   delegate classes from Rust, own the `CADisplayLink`, feed `cpl-app`
   directly. More code than 1, no Swift at all, full control.
3. **A Swift host compiled by `swiftc` from a script.** Switchblade's plan: a
   `cbindgen` C header over `cpl-app` + `cpl-render`, a handful of Swift
   files, built with `xcrun -sdk iphoneos swiftc` and assembled into a bundle
   by `ios/build-ios.sh`. Most conventional; the only one that adds a second
   language.

Whichever wins, the bundle is assembled by a script: `cargo build --target
aarch64-apple-ios`, `Info.plist`, the binary, `codesign` with a development
identity and a provisioning profile, then `xcrun simctl install` for the
simulator or `xcrun devicectl device install app` for a device. Signing and
provisioning are the part of iOS that resists scripting; the spike measures
how much. `xtool` (an open-source SwiftPM-to-iOS-app builder) is worth an
hour's evaluation as a fourth path, but it is not depended on.

`cpl-app` keeps its rule: no winit, no wgpu, no UIKit types. On iOS the
config is a bundled TOML string plus a device-class overlay, no hot reload,
exactly as switchblade's proposal puts it.

## Unknowns to manage

Ranked by how much they could change the design. Each names the spike or
measurement that settles it.

1. **Text quality across both polarities.** CoreText coverage blended on a
   non-sRGB surface gives a different stem weight for white-on-black than for
   black-on-white; Apple's own text system compensates. The look the brief
   asks for lives or dies on this. *Spike:* render the same page both ways
   beside an `NSTextView` at 1x and 2x, compare stem weight by eye and by a
   pixel diff, and if needed apply a per-polarity coverage curve in the text
   shader. Also decide here whether CoreText-atlas or a GPU vector text stack
   (`vello` with `parley`) is the better reference; the expectation is that
   CoreText wins on small body text and the vector stack is not needed.
2. **The readability floor.** Points per preset is a guess until a tablet
   has sat on a stand under stage light. *Measurement:* the fit-coverage
   metric (below) across the corpus at each preset, then a rehearsal.
3. **Fit coverage of a real corpus.** How many of the user's songs fit on an
   iPad mini at `stand`, and at which ladder level? If most songs land at
   step 9 or 10 the premise weakens and the paged mode becomes primary.
   *Measurement:* a headless `cpl fit-report <folder> --device ipad-mini`
   subcommand built in Phase 1, before any window exists. chordpro-pdf's
   device presets (logical point sizes) are the device table.
4. **Atlas capacity at retina body sizes.** A whole song at 17 pt on a 2x
   iPad Pro is a few hundred strings of up to ~1,400 device px; a 2048² R8
   atlas may not hold two songs. *Decision:* start at 4096² (16 MiB, fine on
   both platforms) and keep switchblade's refuse-never-reset rule; measure
   residency with the pre-rasterised neighbour songs.
5. **Karaoke timing sources.** Line following on a pedal is certain. Whether
   LRC alignment is good enough to be a feature rather than a toy depends on
   the corpus having LRC files that match these charts. *Spike:* align
   twenty songs' LRC files to their charts and count the lines that land.
6. **Lenient import of scraped tabs.** The share of the corpus that parses
   clean decides how much the detection pass matters. *Measurement:* the
   fit-report subcommand also reports parse warnings per file.
7. **The iOS host.** Which of the three candidates, and how much of signing
   can be scripted. *Spike:* a Metal triangle from Rust on a device, built
   and installed without opening Xcode, through each candidate in turn until
   one works in a day.
8. **Fonts.** System faces only (SF Pro, New York for a serif body, the
   system monospaced face for tablature) are free on both platforms and need
   no licence; a "manuscript" look may want a specific face. *Decision:*
   ship system faces, make the face a config key, and let a bundled font be
   a later addition through the same `TextRaster`.
9. **Delegated environments and images.** `abc` and `ly` blocks are rendered
   by the reference implementation through external programs (abcm2ps,
   LilyPond); an `svg` block needs a vector rasteriser. None of that belongs
   in a stage viewer's frame path. *Decision to make in Phase 1:* either
   rasterise `svg` through `resvg` on the worker and show `abc`/`ly` as a
   labelled placeholder, or pre-render all three at library-scan time into
   the cache. Images follow the same worker-rendered, cached path.
10. **Measurement on the worker versus the frame.** The layout cache assumes
   a `Page` is always ready; the first open of a cold song is not. *Rule:*
   the first frame draws the header and a `laying out…` line, never a
   half-measured page, the same "pending state" shape as switchblade's
   `MetaLookup::Pending`.

## Settled decisions

These are not re-litigated without the user:

- **ChordPro 6 in full, measured against the reference implementation.** The
  parser is new, written to the spec, with a conformance note that lists
  every construct and its status. Formatting directives are accepted and
  ignored by design; everything else in the spec is honoured.
- Rust, `wgpu`, `winit` on macOS; the switchblade `App` boundary; the
  `schedule.rs` redraw policy copied with its tests, not re-derived.
- CoreText whole-string rasterisation; no font crate; no bundled fonts at
  launch.
- Em-unit layout with a readability floor in points and the ladder above;
  fit never draws under the floor.
- Black and white only, as a polarity toggle. No colour themes.
- Sidecar metadata; the app never writes a song file.
- No Xcode project for either platform; scripts assemble both bundles.
- Config is a complete default TOML overlaid by a user file with unknown keys
  rejected (abner), feel and layout constants live in it (switchblade), and
  it hot-reloads on macOS.
- Not in scope: editing, PDF export (cheap later because the layout is
  shared, as chordpro-pdf proved), setlists beyond folder order (likely wanted
  soon, not before the page is right), audio following, cloud sync beyond
  what iCloud Drive does to a folder, Android, Linux, Catalyst, SwiftUI.

## Verification

- `cpl-song` and `cpl-layout` test on any machine: the parser suite,
  transpose spelling, ladder order, fit determinism, and property tests that
  no `Page` ever places a lyric run below the floor or outside the viewport.
- **Conformance against the reference implementation.** A `cpl conform
  <folder>` subcommand runs the official `chordpro` program and this parser
  over the same files and diffs structure, metadata, chords and lyric text.
  It runs in CI over the spec's examples and the testdata, and on demand over
  the corpus. The conformance note's status table is generated from its
  output, so the table cannot drift from the code.
- **Golden renders.** A headless `cpl render song.cho --device ipad-mini
  --out page.png` draws through the real wgpu pipeline to an offscreen
  texture, chordpro-tui's `--print` for pixels. Goldens are diffed per
  commit; a change to layout constants shows up as an image diff a human can
  judge. This is also how the polarity spike is scored.
- **Fit report.** `cpl fit-report` over the corpus prints a table of songs ×
  devices with ladder level and body size. It is the product metric and runs
  in CI.
- A `--view` flag reaches every surface for window captures (abner's rule),
  and `scripts/window-id.swift` plus `screencapture -l` takes them without
  injected keystrokes.
- Frame-path rules get drift guards as switchblade gives them: a test that
  the render thread never calls into the filesystem during `frame()`, a test
  that a resize does not rasterise more than the budget per frame, a test
  that changing polarity mints no new atlas entries.
- iOS: the Tier A equivalent is that `cpl-app` runs headless on the Mac with
  the device-class overlay, so every layout and ladder test covers the iPhone
  configuration without a device.

## Phases

Each phase ends with something that runs. Nothing is promised before Phase 0
reports.

**Phase 0 — spikes, two weeks, in parallel.**
Text polarity A/B (unknown 1). Fit-coverage prototype over the corpus using a
throwaway CoreText measurer (unknowns 2, 3, 6). Metal triangle on an iPhone
without opening Xcode, through the three host candidates (unknown 7).

**Phase 1 — the pure core.** `cpl-song` written to the ChordPro 6 spec with
the `conform` subcommand and the reference implementation installed, the TUI
tests as the regression floor, the chord grammar and note-name systems, the
grid environment. `cpl-layout` with the em table, fit search, ladder, `Page`. The `Measure`
trait with a CoreText impl and a fake for tests. The `fit-report` and
`render` subcommands. Golden renders of the testdata. No window yet.

**Phase 2 — the Mac page.** `cpl-render` (quads, text atlas, spans) and the
winit host: fit, scroll, sync and paged modes, transpose, polarity, folds,
keys and pedal, keep-awake, chromeless fullscreen, config and theme files
with hot reload, the `.app` script. A song is on a stand.

**Phase 3 — the library.** Worker scan, metadata cache, search surface with
dropping columns and filter pills, tags and ratings in the sidecar, next and
previous pre-layout, the chord viewer. Open With.

**Phase 4 — the singer.** Lyric-first option, line following by clock and
pedal, LRC alignment, tap timing, the word sweep.

**Phase 5 — iOS.** The chosen host, touch (tap zones for next/prev line and
song, pinch for scroll-mode size, swipe for song), Files picker and bookmarks,
device-class overlay, lifecycle, TestFlight.

## Risks, ranked

1. **The page does not look like a manuscript.** If grayscale coverage on a
   GPU quad cannot match native text in both polarities, the vector text
   stack is the fallback and costs weeks. Phase 0 exists for this.
2. **The premise fails on the corpus.** If fit coverage at `stand` is poor,
   the paged mode is promoted and fit becomes the short-song case. The
   fit-report says so in Phase 1, before the window exists.
3. **winit on iOS is not ready and the Rust UIKit host is a long tail.** The
   Swift-by-script host is the fallback and is known to work from
   switchblade's planning.
4. **Scope creep toward an editor.** Tap timing, tags and ratings are all
   "just one write" away from touching the file. The sidecar rule holds.
5. **Setlists.** The first rehearsal will ask for them. They are a list of
   fingerprints in the sidecar and a next/prev order; they are kept out of
   the phases above so the page is finished first, and they are the first
   thing after Phase 3.
