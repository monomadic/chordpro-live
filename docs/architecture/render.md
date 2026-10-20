# macOS renderer

`src/main.rs` owns the winit event loop and the wgpu surface. `cpl-render`
accepts the surface and a headless `Frame`, and has no winit dependency.
On page changes it rasterises whole strings through the CoreText shim,
composites a monochrome RGBA page, and uploads that texture to wgpu. Ordinary
redraws only draw and present a fullscreen triangle. Resize and the `i`
polarity key rebuild the page; Escape closes the window.
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
The rasterisation currently happens in the window callback; moving it off
the frame path is required before making the live-performance no-stall claim.
