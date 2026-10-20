# macOS renderer

`src/main.rs` owns the winit event loop and the wgpu surface. `cpl-render`
has no winit dependency. A worker uses `PageCompositor` to rasterise whole
strings through the CoreText shim and compose a complete monochrome RGBA page.
The window callback uploads only the newest completed image and presents it
with a fullscreen triangle. Resize and the `i` polarity key request a new
page; the previous texture remains visible until that page is ready. Escape
closes the window.
Coverage rasters are cached by text, face, weight, size and display scale,
with a 32 MiB cap. Scrolling still composites and uploads the visible page.

The page uses the system proportional face for title, byline and lyrics,
and the system monospaced face for chord symbols, section labels and status.
Measurement and rasterisation receive the same face, size and weight, so
switching faces preserves chord alignment. The type scale and spacing live
in `config/default.toml`.

`Measure` for `CoreText` is `sb_text_width`: the line's typographic advance,
trailing spaces included, with no bitmap drawn. A raster carries one pixel of
slack on each side (`RASTER_PAD`); compositing places the glyph origin, not
the bitmap's edge, at the item's `x`.
Text is rasterised with the font at its point size on a context scaled to
the display, never as a larger font: SF Pro's tracking and optical size
depend on the point size, so a 17 pt run requested as 34 pt for a 2x display
draws about 5% narrower than measured.
`tests/page_bounds.rs` checks actual nonzero CoreText coverage against fitted
page bounds at 1x and 2x for several viewport sizes. It includes header,
section label, lyric and chord runs.

The CoreText shim is copied from
`switchblade/crates/sb-window/src/text_shim.m`. This starter does not yet
use switchblade's R8 atlas, span highlighting, raster budget, or scheduler.
Page composition and rasterisation run on a worker. Full texture upload still
occurs for every accepted scroll step, so scroll latency and GPU transfer
cost remain to be measured and reduced.

The worker also owns the `Setlist`. Whenever no request is waiting it does
one unit of preparation and checks again: measure a missing neighbour song,
or compose the next song's first page at the last viewport and polarity.
A song-change request that matches that preloaded page (same song, size,
scale, polarity, offset zero) is answered with it directly; anything else
composes as usual. Only the newest generation is accepted, so a page from
the previous song can never land after the change. `--song N` opens the
viewer on a given song and `scripts/window-id.swift` finds the window, so a
capture of any song needs no keypress.
