# chordpro-live — agent notes

A GPU-rendered ChordPro **viewer** for live performance, Rust, macOS and iOS.
One screen holds the whole song, black on white or white on black, like a
manuscript page; when a song cannot fit at a readable size the app compacts in
a fixed order and then scrolls or pages, never squeezes. Lyrics outrank chords.
It is not an editor and never writes a `.cho` file.

## Where things are written down

| File | Holds |
|---|---|
| [PROPOSAL.md](PROPOSAL.md) | Design decisions, ranked unknowns, phases. The source of truth: read the relevant section before structural work, do not re-litigate a settled decision without the user, and change it in the same commit as any decision. |
| [TASKS.md](TASKS.md) | Open work only, by epic. |
| [HISTORY.md](HISTORY.md) | Shipped work, one dated entry per finished epic or task. |
| `docs/architecture/` | One specification per subsystem: [song](docs/architecture/chordpro.md), [layout](docs/architecture/layout.md), [app](docs/architecture/app.md), [render](docs/architecture/render.md), [library](docs/architecture/library.md). Specs, not history; add a note as each subsystem lands. |
| [assets/icons/README.md](assets/icons/README.md) | The app icon and its alternatives. |

## Status

A macOS viewer is built; the iOS host is not.

- **`cpl-song`** parses ChordPro 6 to the reference's parse in all but one
  probed construct: grids, annotations, selectors, `%{}` substitution and
  multi-song files included ([coverage](docs/architecture/chordpro.md)).
  Live transpose follows chordpro-tui's spelling rules.
- **`cpl-layout`** measures each song at the floor and larger point sizes
  into em-unit runs, fits whole sections into columns, and walks the
  reduction ladder.
- **`cpl-app`** composes the page from those runs without measuring again,
  and owns the library, search, the `library.toml` sidecar and the
  `Setlist`.
- **`cpl-render`** composites CoreText rasters on a worker into a retained
  page slab that the shader places by a view uniform, so a scroll step
  uploads only the status strip.
- **`src/main.rs`** hosts the chromeless winit window and owns the page
  worker. Several songs or a folder open as an ordered setlist; the next
  song is composed while the worker is idle, and a song change shows the
  old page until the new one is complete.

On the page: a header with metadata pills and a chord diagram for every
chord used, chord names in the Theme menu's accent; tablature as small, soft staves that wrap into systems at bar
lines (ladder step 5 folds it, `t` pins it); an overflowing song scrolls or,
with `v`, turns pages.

Search (`/`, `o` or ⌘F) covers the library, which is also the home screen
on a bare launch: the folders named on the command line, else the config's
`[library] folders`, else the current folder, else `~/Music/ChordPro` for a
Finder launch (which starts in `/`). The library is scanned on the worker,
rescanned while search is open, and fingerprinted by content. Ratings, tags
and when each song was last shown live in the sidecar; opening a row makes
the results the setlist ([library](docs/architecture/library.md)).

**Still planned — do not describe as implemented:** saved setlists, full
ChordPro 6 conformance (see TASKS.md), a text atlas, the iOS host, and
Finder document opening for the `.app`.

## Working from the task list

**[TASKS.md](TASKS.md) is where work comes from.** When you need something
to do, take the first unchecked task in the highest open epic there; do not
invent work the list does not name without the user.

- A task is checked off only after the real window check and the automated
  checks it names have passed.
- **When every task in an epic is checked, remove the epic from TASKS.md and
  record it in [HISTORY.md](HISTORY.md)** as one dated entry naming the
  epic, what shipped, what was checked, and anything left unverified.
- New work found while doing a task goes into TASKS.md under the right
  epic, not into the commit message alone.

## Build and check

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

- **Visual goldens:** `cargo test --test visual` compares eighteen headless
  renders with `tests/goldens/`. After an intended visual change, inspect
  `target/visual/` and re-bless with `CPL_BLESS=1 cargo test --test visual`.
- **Conformance:** `cargo run -- conform` compares `cpl-song`'s parse with
  the reference `chordpro` (`cpan App::Music::ChordPro`, found via `$CPL_CHORDPRO`,
  `PATH` or `~/perl5`), construct by construct and file by file. After a
  parser change, regenerate the song note's table with
  `cargo run -- conform --table docs/architecture/chordpro.md`;
  `tests/conform.rs` fails while it is stale. Never hand-edit that table.
- **Launch:** `scripts/launch-check.sh` launches the viewer repeatedly and
  fails if a page was never presented. `CPL_TIMING=1` logs worker answers,
  accepted pages and presents to stderr, in milliseconds from process start.
- **Isolation:** tests and captures must pass
  `--store tests/fixtures/library.toml` (or set `$CPL_LIBRARY_STORE`) and
  `--config tests/fixtures/config.toml` (or `$CPL_CONFIG`), so they never
  touch the user's sidecar or config in Application Support.

## Running the viewer

The binary is `chordpro-live`; `chordpro-live --help` lists every form.

| Command | Opens |
|---|---|
| `cargo run` | The library: the current folder, or the config's `[library] folders` |
| `cargo run -- song.cho` | One song |
| `cargo run -- a.cho b.cho` | The first song, the rest queued after it as a setlist |
| `cargo run -- testdata` | A folder's songs as a setlist |
| `cargo run -- page` | The sample page (`page-dark`, `page-inline` inline chords, `page-scroll` overflow) |
| `cargo run -- render song.cho --out page.png --scale 1 --size 420x720` | Nothing: writes exactly what the window would show, headless; use it for 1x checks |
| `cargo run -- print song.cho` | Nothing: prints the parsed lyrics |

Options follow the songs:

| Option | Effect |
|---|---|
| `--dark` | White on black |
| `--size 980x720` | A repeatable window size in logical points |
| `--song 2` | Open on the second song of the setlist |
| `--scroll 120` | Open scrolled that many points |
| `--paged`, `--page 2` | Open in paged mode, optionally on a page |
| `--fullscreen` | Open in fake fullscreen, as `f` toggles it |
| `--tabs show\|fold` | Pin tablature (`testdata/greensleeves.cho` has long tabs) |
| `--transpose 2` (or `-3`) | Open transposed, as `]` or `[` would |
| `--accent purple` | Open with that Theme menu accent (`Blue` … `Teal`, or `Ink`) |
| `--search ris [--open 2 [--next 1]]` | Open in search with a query typed; open the second result as Enter would; step through the results as `n` does |
| `--store lib.toml` | Use another sidecar (or `$CPL_LIBRARY_STORE`) |
| `--floor 10` | Set the readability floor in points |
| `--config my.toml` | Lay a `[page]`/`[library]` table over `config/default.toml` (else `$CPL_CONFIG`, else `config.toml` in Application Support) |

Keys in the viewer:

| Key | Action |
|---|---|
| `f` | Fake fullscreen (simple fullscreen over the desktop, as switchblade's `fast_fullscreen`) |
| `i` | Polarity |
| `e` | Open the current file in `$EDITOR` (or `vi`) in Terminal; reopen from search after saving |
| `[` `]`, `0` | Transpose a semitone down or up; reset |
| `v` | Scroll or paged |
| `t` | Fold or show tablature |
| `n` `p`, ← → | Next and previous song |
| ↑ ↓, Space, Page Up/Down, Home, End | Scroll or turn pages |
| `/`, `o`, ⌘F | Search: digits rate, `#` tags, Tab sorts, Enter opens, Escape returns. The query edits as a Mac text field (← →, ⌥ and ⌘ arrows, ⌥⌫, ⌘⌫, Ctrl-A E B F W U K D N P, ⌘V ⌘C ⌘X) |
| `q`, Escape, ⌘Q | Quit |

In search the pointer works too: moving onto a row focuses it (only a
move does, so a resting pointer never takes the keyboard's row), a click
opens it, and the wheel or trackpad scrolls the list. The **Theme** menu in
the menu bar sets the chord accent.

**Window captures.** Verify visual changes with a capture reached through
launch flags, never by injecting global keystrokes:

```bash
screencapture -x -l "$(swift scripts/window-id.swift | head -1)" shot.png
```

**App bundle.** `packaging/build-app.sh` builds
`target/release/bundle/ChordProLive.app`: the release binary, `Info.plist`
from `packaging/Info.plist.in` and `packaging/version.sh`,
`assets/icons/icon-01.icns` as its icon, ad-hoc signed and registered with
LaunchServices. `--install` copies it to /Applications and `--open`
launches it; launched from Finder it opens `~/Music/ChordPro`, made on first
launch. The repo folder carries a `com.apple.macl` privacy label, so macOS
may draw the bundle in `target/` with a blank icon; a copy outside the repo
shows the real one.

## Design source

Every surface is mocked in Figma —
[chordpro-live — Screens](https://www.figma.com/design/0ovWyKEx7qyWh38a08Wl82)
— and PROPOSAL.md's "Design source" section says how the file maps to the
code (Polarity variables, `Page/*` text styles, segment auto-layout). Keep
the mock and the em table in `config/default.toml` in step: a layout
constant that changes in one changes in the other.

## Sibling projects

Local checkouts this project borrows from; their notes document the
rationale behind what is reused here:

| Project | Path | Borrowed |
|---|---|---|
| chordpro-tui (Go) | `~/src/chordpro-tui` | parse → Song → render pipeline, fit search, reduction ladder, scroll modes, finder, transpose, the PDF exporter's em-unit layout |
| switchblade (Rust) | `~/src/switchblade` | `App` trait boundary, `schedule.rs`, `Waker`, CoreText text atlas, span highlights, config/theme hot reload, `.app` packaging, the iOS proposal |
| abner (Rust) | `~/src/abner` | slim crate layout, complete-default TOML config, SDF quad pipeline, launch flags, chromeless window shims |
| chordpro-library | `~/src/chordpro-library` | the local ChordPro 6 working specs |

chordpro-tui is the original ChordPro app this viewer grows out of: a Go
terminal renderer that already fits a whole song on one screen, with a fit
search, reduction ladder, transpose, chord diagrams, a song finder, and a
one-page PDF exporter. Most layout and product questions here were first
answered there, so read its `README.md`, `doc/`, and `internal/` before
designing a feature it already has, and treat its behaviour as the baseline
this app should match or consciously improve on.

When porting something, port its tests with it and name the source file in
the commit message.

## Product and architecture rules

- **ChordPro 6 in full is the target.** Extend the parser against the spec
  (chordpro.org) and measure it with `conform`. Formatting directives are
  accepted and ignored by design; everything else must be honoured.
- **The page is always readable.** Body text never drops below the
  readability floor. The ladder (`LADDER` in `cpl-layout/src/ladder.rs`)
  compacts least destructive first: 1 tight sections, 2 side labels,
  4 chord diagrams, 5 fold tablature, 6 collapse repeats, 7 inline chords,
  8 metadata pills, 9 title (step 3 was removed; the numbers stand). When
  every step fails, the window scrolls the full song at the floor. Lines
  too wide for the window wrap at word boundaries; only an unbreakable run
  reaches the width-needed state.
- **Nothing surprising happens during a song.** No reflow, no atlas blink, no
  I/O on the frame path, no screen sleep. Page measurement and CoreText
  rasterisation run on a worker, which preloads the next song while idle;
  the page is retained as a slab and a scroll step uploads only the status
  strip. The render thread must never touch the filesystem.
- **Sidecar metadata only.** Tags, ratings, timings and setlists live in the
  library sidecar keyed by content fingerprint. The app never rewrites a
  song file.
- **No winit, wgpu or UIKit types in `cpl-app`.** That boundary is what makes
  the iOS host a second caller rather than a port.
- **Xcode-minimal.** macOS needs only the Command Line Tools; bundles are
  built by scripts in `packaging/` (and `ios/`, planned). No `.xcodeproj`,
  ever.
- **Black and white first**, as a polarity toggle; colour is used
  sparingly, never as decoration. Chord names take the Theme menu's accent
  (`THEMES` in `cpl-app`, default `#007AFF`, Ink restores monochrome) and
  search matches sit on a highlighter yellow. Every ink, colour included,
  has a light and a dark value.
- Keep dependencies lean; ask before adding one.

## Conventions

- Rust edition 2024, rustfmt defaults, clippy clean with `-D warnings`.
- Feel and layout constants live in `config/default.toml`, never hardcoded.
- Commit messages say what changed for the user, then why; cite the sibling
  file when something was ported.
