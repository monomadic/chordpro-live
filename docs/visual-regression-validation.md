# Validation record — 21 October 2026

Implementation base: `be77582fad36d445fa2e566305d4dda0705d786f`.
Environment: macOS 26.5, arm64, restricted agent shell.

- `cargo test --workspace`: passed, 85 tests total, including two new CoreText
  fixture tests and the existing 1x/2x raster bounds test.
- `cargo fmt --all -- --check`: passed.
- `python3 scripts/visual-regression.py --list`: passed; 15 named cases.
- Swift capture helper: compiled successfully with the available macOS SDK.
- `cargo clippy --workspace --all-targets -- -D warnings`: failed on six existing
  `clippy::chunks_exact_to_as_chunks` findings in
  `crates/cpl-render/src/lib.rs` (93, 175, 541, 584, 587, 588), under Rust 1.98.
  No renderer code was changed for this workflow.

A real `scarborough-light` launch was attempted. The viewer logged an invalid
connection to `com.apple.hiservices-xpcservice` and failed to produce a worker
slab within 30 seconds. The capture helper independently reported that Screen
Recording permission was unavailable. No screenshot was produced or reviewed.
The runner now performs permission/display preflight before launching viewers.
The failed run manifests and logs were retained under
`/tmp/cpl-visual-20261021-retry` and `/tmp/cpl-visual-20261021-final`; these are
local diagnostics, not durable visual evidence or approved baselines.

Pending: both original window checks, every matrix image review, 1x/2x display
verification, actual contrast/readability assessment, and baseline approval.
Neither the lyric-wrapping task nor the visual-regression task is checked off.
