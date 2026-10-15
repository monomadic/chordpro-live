# chordpro-live — agent notes

A GPU-rendered ChordPro **viewer** for live performance, Rust, macOS and iOS.
One screen holds the whole song, black on white or white on black, like a
manuscript page; when a song cannot fit at a readable size the app compacts in
a fixed order and then scrolls or pages, never squeezes. Lyrics outrank chords.
It is not an editor and never writes a `.cho` file.

## Status

**Proposal only. Nothing is built.** [PROPOSAL.md](PROPOSAL.md) is the source
of truth for every design decision, the ranked unknowns and the phases. Read
the relevant section before structural work, and do not re-litigate a
decision listed under "Settled decisions" there without the user. When a
decision changes, change PROPOSAL.md in the same commit.

Once code exists this file grows the architecture map, the build and
verification commands, and one `docs/architecture/<subsystem>.md` note per
subsystem, written as each lands and kept as specification, not history.

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

## Rules that already hold

- **ChordPro 6 in full.** The parser is written to the current spec
  (chordpro.org) and measured against the reference implementation by the
  `conform` subcommand. Formatting directives are accepted and ignored by
  design; everything else is honoured. The conformance note's status table
  is generated, never hand-edited.
- **The page is always readable.** Body text never drops below the
  readability floor; fit descends the ladder, then switches mode. No layout
  ever draws under the floor.
- **Nothing surprising happens during a song.** No reflow, no atlas blink, no
  I/O on the frame path, no screen sleep. The render thread never touches the
  filesystem.
- **Sidecar metadata only.** Tags, ratings, timings and setlists live in the
  library store keyed by content fingerprint. The app never rewrites a song
  file.
- **No winit, wgpu or UIKit types in `cpl-app`.** That boundary is what makes
  the iOS host a second caller rather than a port.
- **Xcode-minimal.** macOS needs only the Command Line Tools; bundles are
  assembled by scripts in `packaging/` and `ios/`. No `.xcodeproj`, ever.
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
