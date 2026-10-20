# macOS renderer

`src/main.rs` owns the winit event loop and the wgpu surface. `cpl-render`
has no winit dependency. A worker uses `PageCompositor` to rasterise whole
strings through the CoreText shim and compose a monochrome RGBA **slab**: the
rows of the positioned page around the current offset, at most
`SLAB_VIEWPORTS` (3) viewports tall and never past the adapter's texture
limit, so a 980×720 pt window at 2x keeps about 35 MB resident however long
the song. The window callback uploads a slab once (slabs carry an id) and
presents it with a fullscreen triangle that reads the slab, a separate
one-line status texture and a 32-byte `View` uniform: the slab row shown at
the top, the band of content rows outside which it paints paper (so a
partially visible chord/lyric group never shows), the row the status strip
starts on, the title-strip inset, and the paper level. A scroll step inside
the slab therefore uploads only the status strip and the uniform; a step
that leaves it composes the next slab from the retained frame without
measuring. Resize and the `i` polarity key retain a new page; the previous
texture remains visible until the new one is ready. `q` and Escape close the
window.

**The window is chromeless**, with winit's own macOS attributes rather than
an AppKit shim: transparent titlebar, hidden title, full-size content view
and no shadow — on macOS 26 the window contour is drawn with the shadow, so
dropping the shadow is what removes it (switchblade's finding). The page
runs under a `TITLE_STRIP_PT` (28 pt) strip of paper so the traffic lights
float over it; the strip is part of the view key and shrinks the page's
logical height. It is a constant until fullscreen exists, where it must be
zero.
Coverage rasters are cached by text, face, weight, size and display scale,
with a 32 MiB cap.

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
Page composition and rasterisation run on a worker. `CPL_TIMING=1` logs each
answer as `slab` or `view` with its wall time; in a debug build a slab takes
50–120 ms and a view 2 µs for a fitted page or 3–9 ms for a scrolling one
(the status strip raster). Release timings and the upload cost itself are
still to be measured.

The worker also owns the `Setlist`. Whenever no request is waiting it does
one unit of preparation and checks again: measure a missing neighbour song,
or compose the next song's first page at the last viewport and polarity.
A song-change request that matches that preloaded page (same song, size,
scale, polarity, offset zero) is answered with it directly; anything else
composes as usual. Only the newest generation is accepted, so a page from
the previous song can never land after the change. `--song N` opens the
viewer on a given song and `scripts/window-id.swift` finds the window, so a
capture of any song needs no keypress.
