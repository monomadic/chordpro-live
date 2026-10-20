# chordpro-live — agent notes

A GPU-rendered ChordPro **viewer** for live performance, Rust, macOS and iOS.
One screen holds the whole song, black on white or white on black, like a
manuscript page; when a song cannot fit at a readable size the app compacts in
a fixed order and then scrolls or pages, never squeezes. Lyrics outrank chords.
It is not an editor and never writes a `.cho` file.

## Status

An initial macOS viewer is built. `cpl-song` parses a small ChordPro slice;
`cpl-layout` fits whole sections into columns at the stand-size floor;
`cpl-app` composes the page; `cpl-render` composites CoreText
rasters into a wgpu texture; `src/main.rs` hosts the winit window. The
remaining reduction steps, scrolling/paging, library, full ChordPro 6
conformance, text atlas, and iOS host are still planned. Do not describe
these as implemented.

[PROPOSAL.md](PROPOSAL.md) remains the source of truth for design decisions,
ranked unknowns, and phases. Read the relevant section before structural
work; do not re-litigate a settled decision without the user. When a
decision changes, change the proposal in the same commit.

Architecture notes: [song](docs/architecture/chordpro.md),
[layout](docs/architecture/layout.md), [app](docs/architecture/app.md),
[render](docs/architecture/render.md).
Add a `docs/architecture/<subsystem>.md` note as each subsystem lands,
and keep these notes as specifications rather than history.

Build with `cargo build --workspace`. Check with `cargo test --workspace`
and `cargo clippy --workspace --all-targets -- -D warnings`. Run the
sample macOS page with `cargo run -- --view page` (or `page-dark`),
or open a song with `cargo run -- --view song.cho`. Press `i` for polarity
and Escape to close. `cargo run -- song.cho` prints the parsed lyrics.

## Design source

Every surface is mocked in Figma — [chordpro-live — Screens](https://www.figma.com/design/0ovWyKEx7qyWh38a08Wl82)
— and PROPOSAL.md's "Design source" section says how the file maps to the
code (Polarity variables, `Page/*` text styles, segment auto-layout). Keep the
mock and the em table in step: a layout constant that changes in one changes
in the other.

## Sibling projects

Three local checkouts this project borrows from; their notes document the
rationale behind what is reused here:

| Project | Path | Borrowed |
|---|---|---|
| chordpro-tui (Go) | `~/src/chordpro-tui` | parse → Song → render pipeline, fit search, reduction ladder, scroll modes, finder, the PDF exporter's em-unit layout |
| switchblade (Rust) | `~/src/switchblade` | `App` trait boundary, `schedule.rs`, `Waker`, CoreText text atlas, span highlights, config/theme hot reload, `.app` packaging, the iOS proposal |
| abner (Rust) | `~/src/abner` | slim crate layout, complete-default TOML config, SDF quad pipeline, `--view` flags, chromeless window shims |
| chordpro-library | `~/src/chordpro-library` | the local ChordPro 6 working specs |

When porting something, port its tests with it and name the source file in
the commit message.

## Product and architecture rules

- **ChordPro 6 in full is the target.** The current parser is not conformant.
  Extend it against the spec (chordpro.org), then measure it against the
  reference implementation with a `conform` subcommand. Formatting
  directives are accepted and ignored by design; everything else must be
  honoured. Generate the conformance note's status table; never hand-edit it.
- **The page is always readable.** Body text never drops below the
  readability floor. The current page shows a paging-needed message when
  it overflows after trying the first three applicable reduction steps.
  The remaining ladder must descend before switching mode.
- **Nothing surprising happens during a song.** No reflow, no atlas blink, no
  I/O on the frame path, no screen sleep. The current renderer rasterises on
  page updates inside the window callback; move that work off the frame
  path before claiming this rule is met. The render thread must never touch
  the filesystem.
- **Sidecar metadata only.** Tags, ratings, timings and setlists live in the
  planned library store keyed by content fingerprint. The app never rewrites
  a song file.
- **No winit, wgpu or UIKit types in `cpl-app`.** That boundary is what makes
  the iOS host a second caller rather than a port.
- **Xcode-minimal.** macOS needs only the Command Line Tools. Bundle scripts
  in `packaging/` and `ios/` are planned. No `.xcodeproj`, ever.
- **Black and white only**, as a polarity toggle. No colour themes.
- Keep dependencies lean; ask before adding one.

## Conventions

- Rust edition 2024, rustfmt defaults, `cargo clippy --workspace --all-targets
  -- -D warnings` clean.
- Feel and layout constants live in the config file, never hardcoded.
- Verify visual changes with a window capture reached through a `--view`
  flag, never by injecting global keystrokes.
- Commit messages say what changed for the user, then why; cite the sibling
  file when something was ported.
