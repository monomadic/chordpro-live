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

In paged mode the worker answers a page request the same way: it asks the
app for the page's view first, then keeps the slab if it covers that offset
(or reaches the end of the page, for a short last page), so a page turn
uploads only the status strip unless it leaves the slab.

**One CoreText call at a time.** The shim creates fonts through AppKit's
`NSFont`, and measuring on one thread while another rasterised returned wrong
advances. `text.rs` serialises both shim calls behind a process-wide lock;
the app measures and draws on one worker, so it costs nothing there, and
`tests/coretext_threads.rs` guards it. Moving the shim to `CTFont` would
make the lock unnecessary.

**Headless renders.** `cpl render <song|sample> --out page.png` answers one
request through the worker's real path (`Worker::answer`) and runs the
fragment shader's placement rule on the CPU (`present`), so the PNG holds
the pixels the window would present at any `--scale`, `--size`, polarity,
scroll offset or page. `cpl-render/src/png.rs` writes it with stored deflate
blocks and no dependency.

**Visual regression.** `tests/visual.rs` renders each case with the real
binary (`cpl render`) and decodes both it and its golden with
`cpl-render/src/png.rs`. Goldens are `sips`-compressed PNGs in
`tests/goldens/`; a case fails when more than 0.02% of pixels move more than
24 levels in any channel, which tolerates antialiasing drift from a macOS
font update but not a moved run. After an intended visual change, inspect
`target/visual/*.png`, then run `CPL_BLESS=1 cargo test --test visual`.
A layout constant only reaches a golden if some case exercises it; the
crowded-chords fixture exists because no song case crowded its chords.

**A skipped present is retried.** A new window reports its surface as
`Occluded` for its first few frames, so `Gpu::render` can fail to present a
page that arrived early. It returns `Presented::Skipped`, the host sets
`needs_present`, and the page is presented again on
`WindowEvent::Occluded(false)` or, failing that, on a 100 ms idle tick
(`ControlFlow::WaitUntil`), after which the loop returns to
`ControlFlow::Wait`. `Presented::Reconfigure` (a lost or outdated surface)
resizes the surface and requests the page again. With `CPL_TIMING=1` every
present, accept and worker answer is logged with milliseconds since process
start; `scripts/launch-check.sh [--rebuild] [count] [--view …]` launches the
viewer repeatedly and fails if any launch's last accepted page was never
presented.
