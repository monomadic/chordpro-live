# macOS viewer visual regression

This workflow captures **review candidates**, not passing baselines. A successful
capture proves neither correct presentation nor readability. Keep the wrapping
and visual-regression tasks unchecked until the real windows have been reviewed.
No song is changed by the runner, and it sends no global keystrokes.

## Run

From the repository root, with an awake, unlocked macOS graphical session and
Screen Recording permission for the invoking app/terminal:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/visual-regression.py --list
python3 scripts/visual-regression.py --output /tmp/cpl-wrap-candidates
```

The output directory must be new. Select individual cases with repeatable
`--case scarborough-light --case scroll-top-light` options. Output is deliberately
outside the repository; copy reviewed evidence into your chosen durable artifact
store. Keep the manifest and logs beside the PNGs. The manifest records HEAD,
working-tree status, binary/config/input hashes, arguments, logical window bounds,
display scales, image dimensions and image hashes. `captured; review pending`
is never an approval. Failures retain an incomplete manifest and logs and return
nonzero; don't use incomplete runs as baselines.

The runner builds with the lockfile, compiles the capture helper, starts one
viewer at a time, waits for a worker `slab` result (30-second limit), allows 1.5
seconds for presentation, and resolves the window by the launched PID. It checks
window bounds in logical points before targeted, shadow-free capture. It only
terminates its own process. A worker result is not a presentation fence: visually
reject blank, stale, partially drawn, occluded or permission-redacted images.
Move the pointer away from the window before capture. The helper checks Screen
Recording permission and main-display sleep; it cannot establish that the session
is unlocked or the page correctly drawn.

Run separately on a 1x display and a 2x display, keeping each run in a separate
directory. Inspect PNG pixel dimensions against logical bounds (420x720 becomes
840x1440 at 2x). The manifest lists available screens, not the actual window's
backing scale; confirm it from the image dimensions. A display that cannot fit the
requested window should fail the bounds check; use a larger display. Do not
resize images to manufacture 1x evidence from 2x.

## Matrix and review criteria

| Cases | Logical size | Required observations |
| --- | --- | --- |
| scarborough-light/dark | 420x720 | Original real-song check: whitespace wraps, intact words, chords over their original syllables, consistent continuation inset, no right-edge lyric clipping. |
| scroll-top-light/dark | 360x420 | Original overflow check: readable floor, visible song and fixed-size header where it fits, status clear of lyrics. |
| scroll-middle-light/dark | 360x420, offset 120 | Whole chord/lyric groups enter/leave together; no half rows under the header/status mask. |
| scroll-end-light/dark | 360x420, offset 100000 (clamped) | Last group reachable and complete; no bottom clipping or lost last words. |
| alignment-light/dark | 420x720 | Follow C/G/Am/F and the chord inside `syll[Dm]able`; chord and syllable move together. Continuation rows start one body em inward (`wrap_indent = 1`), not one extra indent per row. |
| width-needed-light/dark | 360x420 | Deliberately unbreakable word shows width-needed state; no squeezed or split word presented as a readable song. |
| type-ceiling-light/dark | 980x720 | Short lyrics at 28 pt; title/chords/status remain distinct and unclipped. |
| inline-light | 300x244 | Compact chords stay on their own row above the lyrics. |

Both documented commands are exercised verbatim apart from an explicit zero
scroll offset. The CLI has no dark selector for the built-in scroll sample, so
its dark cases use `tests/visual/page-scroll.cho`, an exact copy of SCROLL_SAMPLE.
Keep that fixture synchronized with the constant in `src/main.rs`. There is no
CLI dark-inline selector; this matrix does not claim dark-inline coverage.

Compare light/dark geometry: line breaks, baselines, chord anchors and insets must
match. Check black/white paper and ink, softer secondary text, thin glyph strokes,
and punctuation at native image scale. Assess readability on the actual display
at normal viewing distance; a zoomed screenshot cannot establish it. The scrolling
fixture exercises the 17 pt comfortable scroll size; the short fixture exercises the 28 pt ceiling.
The automated fixture test verifies both endpoint sizes with real CoreText.
Existing layout tests cover refusal to enlarge type when it adds wrapped rows.

## Review and baseline comparison

1. Capture this revision on each supported scale. Review every row above in its
   actual window and PNG. Record reviewer, date, macOS/build, display/scale,
   exact case names and pass/fail observations in a `review.md` next to the run.
   Explicitly record the two original window checks. Preserve failures too.
2. Only after review, retain the entire candidate directory as an approved
   baseline. Never overwrite it with a later run or approve merely because an
   image file exists. No baseline is committed by this change.
3. Capture a later revision with the same matrix, OS, font environment, display
   scale and configuration. Open same-named PNGs side by side at 100%, or overlay
   them in an image viewer. Check the criteria above and document every changed
   wrap, anchor, inset, edge, glyph and status position. Image SHA differences
   identify changed files but are **not** a pixel/perceptual comparison; metadata
   and OS raster changes can change hashes. This workflow currently uses human
   visual comparison, not an automated screenshot-diff threshold.
4. Promote a new baseline only after explaining and reviewing intended changes.
   If OS/fonts/scale differ, create a separate baseline rather than comparing
   unlike rasters. Task completion requires the relevant automated checks plus
   this review evidence; a capture permission/display failure leaves it open.

## Automated scope

`tests/wrapping_visual.rs` measures the real capture fixtures with CoreText at
the requested logical sizes. It guards the 17–28 pt body range, verifies endpoint
fixtures actually reach 17 and 28 pt, and ensures polarity preserves geometry.
Existing unit tests cover word boundaries, inline wrapping, chord anchoring,
continuation indentation, page composition and unbreakable width-needed cases.
`tests/page_bounds.rs` checks actual CoreText ink extents at 1x and 2x for fitted
pages. None of these tests exercises WindowServer, GPU presentation, a real
scroll interaction, monitor contrast, or human readability.
