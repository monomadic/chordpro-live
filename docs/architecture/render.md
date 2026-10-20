# macOS renderer

`src/main.rs` owns the winit event loop and the wgpu surface. `cpl-render`
accepts the surface and a headless `Frame`, and has no winit dependency.
On page changes it rasterises whole strings through the CoreText shim,
composites a monochrome RGBA page, and uploads that texture to wgpu. Ordinary
redraws only draw and present a fullscreen triangle. Resize and the `i`
polarity key rebuild the page; Escape closes the window.

The page uses the system proportional face for title, byline and lyrics,
and the system monospaced face for chord symbols, section labels and status.
Measurement and rasterisation receive the same face, size and weight, so
switching faces preserves chord alignment. The type scale and spacing live
in `config/default.toml`.

The CoreText shim is copied from
`switchblade/crates/sb-window/src/text_shim.m`. This starter does not yet
use switchblade's R8 atlas, span highlighting, raster budget, or scheduler.
The rasterisation currently happens in the window callback; moving it off
the frame path is required before making the live-performance no-stall claim.
