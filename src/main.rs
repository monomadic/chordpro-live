use cpl_app::{App, Config, Frame, Polarity, Setlist, Tabs};
use cpl_render::{CoreText, Gpu, PageCompositor, PageImage, Presented, View};
use std::{
    env, fs,
    path::Path,
    process::ExitCode,
    sync::{Arc, mpsc},
    task::Waker,
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    event::StartCause,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

const SAMPLE: &str = "{title: The Long Road Home}\n{artist: Sample song}\n{start_of_verse: Verse 1}\n[C]Morning light is [G]falling on the [Am]road\n[F]Every step is [C]bringing us back [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 2}\n[C]When the night is [G]quiet we will [Am]know\n[F]All the roads we [C]travel lead us [G]home\n{end_of_verse}";
const INLINE_SAMPLE: &str =
    "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n[G]Bravo\n[Am]Charlie\n{end_of_verse}";
const SCROLL_SAMPLE: &str = "{title: The Long Road Home}\n{artist: Sample song}\n{start_of_verse: Verse 1}\n[C]Morning light is [G]falling on the [Am]road\n[F]Every step is [C]bringing us back [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 2}\n[C]When the night is [G]quiet we will [Am]know\n[F]All the roads we [C]travel lead us [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}";

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = std::task::Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

/// Height of the title strip the page runs under on macOS (transparent
/// titlebar, hidden title, full-size content view): the traffic lights float
/// over paper and the page starts below them. Zero until fullscreen exists.
#[cfg(target_os = "macos")]
const TITLE_STRIP_PT: f32 = 28.0;
#[cfg(not(target_os = "macos"))]
const TITLE_STRIP_PT: f32 = 0.0;

/// A retained slab is at most this many viewports tall, so the page texture
/// stays bounded however long the song: a 980×720 pt window at 2x keeps
/// at most ~35 MB resident.
const SLAB_VIEWPORTS: u32 = 3;

/// Process start, for `CPL_TIMING` stamps.
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn since_start() -> u128 {
    START.get_or_init(Instant::now).elapsed().as_millis()
}

/// How long a skipped present waits before it is retried, when no
/// visibility change arrives first: switchblade's idle tick.
const PRESENT_RETRY: std::time::Duration = std::time::Duration::from_millis(100);

struct Viewer {
    /// `CPL_TIMING`: log presents and skips, stamped from process start.
    timing: bool,
    /// The last present was skipped (the window was occluded or the surface
    /// timed out), so the page on the GPU has not been shown. A page that
    /// arrives while a new window is still occluded otherwise stays unseen
    /// until the next keypress.
    needs_present: bool,
    config: Config,
    polarity: Polarity,
    /// The tablature choice; `t` unfolds a folded tab or folds a shown one.
    tabs: Tabs,
    /// Whether the accepted page has tablature, and whether it is folded.
    page_tabs: Option<bool>,
    initial_size: (f64, f64),
    scroll_offset: f32,
    scroll_max: f32,
    /// Paged mode turns whole pages instead of scrolling; `v` toggles it.
    paged: bool,
    page: usize,
    page_count: usize,
    /// Index into the setlist; `n`/`p` move it, the worker swaps the page.
    song: usize,
    song_count: usize,
    generation: u64,
    /// The slab the GPU holds, so a scroll step that stays inside it uploads
    /// nothing but the status strip.
    uploaded_slab: Option<u64>,
    max_texture_side: u32,
    requests: mpsc::Sender<PageRequest>,
    results: mpsc::Receiver<ReadyPage>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
}

#[derive(Clone, Copy)]
struct PageRequest {
    generation: u64,
    song: usize,
    /// Window size in device pixels; the page occupies the rows below `inset`.
    width: u32,
    height: u32,
    scale: f32,
    /// Title strip height in points.
    inset: f32,
    offset: f32,
    /// Paged mode's page; `offset` is ignored when set.
    page: Option<usize>,
    polarity: Polarity,
    tabs: Tabs,
    max_texture_side: u32,
}

impl PageRequest {
    fn inset_px(&self) -> u32 {
        (self.inset * self.scale).round() as u32
    }
    /// Device rows the page may use.
    fn content_px(&self) -> u32 {
        self.height.saturating_sub(self.inset_px()).max(1)
    }
    fn logical(&self) -> (f32, f32) {
        (
            self.width as f32 / self.scale,
            self.content_px() as f32 / self.scale,
        )
    }
    fn px(&self, points: f32) -> i32 {
        (points * self.scale).round() as i32
    }
}

/// A composed run of page rows, identified so the host uploads it once.
struct Slab {
    id: u64,
    /// First page row (device pixels) the image holds.
    top: i32,
    image: PageImage,
}

struct ReadyPage {
    generation: u64,
    song: usize,
    title: String,
    max_offset: f32,
    /// Paged mode's page count at this viewport.
    pages: usize,
    /// The offset in points the view shows, so scroll mode can resume there.
    view_offset: f32,
    /// `Some(folded)` when the song has tablature.
    tabs: Option<bool>,
    slab: Arc<Slab>,
    /// A freshly composed status strip, when the page does not carry its own.
    status: Option<PageImage>,
    view: View,
}

/// What a retained page depends on besides the song and the scroll offset.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ViewKey {
    width: u32,
    height: u32,
    scale: u32,
    inset: u32,
    polarity: Polarity,
    tabs: Tabs,
}

impl ViewKey {
    fn of(request: &PageRequest) -> Self {
        Self {
            width: request.width,
            height: request.height,
            scale: request.scale.to_bits(),
            inset: request.inset.to_bits(),
            polarity: request.polarity,
            tabs: request.tabs,
        }
    }
}

/// A song's page for one viewport: the positioned frame and the slab of it
/// currently composed. Scrolling inside the slab re-composes nothing.
struct Retained {
    song: usize,
    key: ViewKey,
    page: Frame,
    page_px: u32,
    slab: Arc<Slab>,
}

struct Worker {
    setlist: Setlist,
    compositor: PageCompositor,
    config: Config,
    retained: Option<Retained>,
    /// The next song's page at the last viewport, built while idle.
    preloaded: Option<Retained>,
    last: Option<PageRequest>,
    next_slab_id: u64,
    timing: bool,
}

/// Composes the slab of `page` around `offset_px`, the device row at the top
/// of the content.
fn compose_slab(
    compositor: &mut PageCompositor,
    next_id: &mut u64,
    page: &Frame,
    page_px: u32,
    request: &PageRequest,
    offset_px: i32,
) -> Arc<Slab> {
    let viewport = request.content_px();
    let height = page_px
        .min(viewport * SLAB_VIEWPORTS)
        .min(request.max_texture_side)
        .max(1);
    let top = (offset_px - viewport as i32).clamp(0, page_px as i32 - height as i32);
    let image = compositor.compose_slab(page, request.scale, request.width, height, top);
    *next_id += 1;
    Arc::new(Slab {
        id: *next_id,
        top,
        image,
    })
}

/// Positions a song's page for the request's viewport and composes its first slab.
fn retain(
    compositor: &mut PageCompositor,
    next_id: &mut u64,
    app: &mut App,
    request: &PageRequest,
    offset_px: i32,
) -> Retained {
    app.polarity = request.polarity;
    app.set_tabs(request.tabs);
    let (width, height) = request.logical();
    let page = app.page(width, height);
    let page_px = ((page.height * request.scale).round() as u32).max(1);
    let slab = compose_slab(compositor, next_id, &page, page_px, request, offset_px);
    Retained {
        song: request.song,
        key: ViewKey::of(request),
        page,
        page_px,
        slab,
    }
}

impl Worker {
    /// Answers one request: a song change or viewport change retains a new
    /// page, a scroll that leaves the slab composes another slab, and any
    /// scroll composes only the status strip. Returns whether a slab was
    /// composed, for the timing log.
    fn answer(&mut self, request: &PageRequest) -> (ReadyPage, bool) {
        if request.song != self.setlist.position() {
            self.setlist.go_to(request.song);
        }
        let key = ViewKey::of(request);
        let mut composed_slab = false;
        // Where the content sits decides which slab is needed, so the view
        // comes first; it reads the prepared page and measures nothing.
        let (width, height) = request.logical();
        let (scroll, pages, tabs) = {
            let app = self.setlist.current(&CoreText);
            app.polarity = request.polarity;
            app.set_tabs(request.tabs);
            let scroll = match request.page {
                Some(page) => app.page_view(width, height, page),
                None => app.view(width, height, request.offset),
            };
            let has_tabs = app
                .song()
                .sections
                .iter()
                .any(|section| section.kind == cpl_song::SectionKind::Tab);
            (
                scroll,
                app.page_count(width, height),
                has_tabs.then(|| app.tabs_folded(width, height)),
            )
        };
        let offset_px = request.px(scroll.offset);
        let fresh = self
            .retained
            .as_ref()
            .is_none_or(|r| r.song != request.song || r.key != key);
        if fresh {
            let preloaded = self
                .preloaded
                .take_if(|p| p.song == request.song && p.key == key);
            self.retained = Some(match preloaded {
                Some(preloaded) => preloaded,
                None => {
                    composed_slab = true;
                    let app = self.setlist.current(&CoreText);
                    retain(
                        &mut self.compositor,
                        &mut self.next_slab_id,
                        app,
                        request,
                        offset_px,
                    )
                }
            });
        }
        let retained = self.retained.as_mut().unwrap();
        let viewport = request.content_px() as i32;
        let slab_end = retained.slab.top + retained.slab.image.height as i32;
        // The last page may start past the page's end less a viewport; a
        // slab that reaches the page's end covers it, the rest is paper.
        let covered = retained.slab.top <= offset_px
            && (slab_end >= offset_px + viewport || slab_end >= retained.page_px as i32);
        if !covered {
            composed_slab = true;
            retained.slab = compose_slab(
                &mut self.compositor,
                &mut self.next_slab_id,
                &retained.page,
                retained.page_px,
                request,
                offset_px,
            );
        }
        let retained = self.retained.as_ref().unwrap();
        let scale = request.scale;
        let paper = if request.polarity == Polarity::Light {
            255
        } else {
            0
        };
        let mut view = View {
            scroll: (scroll.offset * scale).round() as i32 - retained.slab.top,
            band_top: (scroll.band.0 * scale).floor() as i32,
            band_bottom: (scroll.band.1 * scale).ceil() as i32,
            status_top: i32::MAX,
            inset: request.inset_px() as i32,
            paper,
        };
        let status = scroll.status.map(|mut text| {
            let c = &self.config;
            let strip = c.margin_bottom + c.status_height * c.body_size;
            let strip_px = (strip * scale).ceil() as u32;
            view.status_top = request.content_px() as i32 - strip_px as i32;
            text.baseline -= height - strip;
            let frame = Frame {
                width,
                height: strip,
                polarity: request.polarity,
                text: vec![text],
                shapes: Vec::new(),
                fits: false,
                columns: 1,
                ladder_step: 10,
            };
            self.compositor
                .compose(&frame, scale, request.width, strip_px)
        });
        (
            ReadyPage {
                generation: request.generation,
                song: request.song,
                title: self.setlist.title(request.song).to_owned(),
                max_offset: scroll.max_offset,
                pages,
                view_offset: scroll.offset,
                tabs,
                slab: Arc::clone(&retained.slab),
                status,
                view,
            },
            composed_slab,
        )
    }

    /// Measures a missing neighbour or retains the next song's first page at
    /// the last viewport. One unit of work per call, so a request never waits
    /// behind more than one of them. Returns false once nothing is left.
    fn preload_step(&mut self) -> bool {
        if self.setlist.preload(&CoreText) {
            return true;
        }
        let Some(last) = self.last else {
            return false;
        };
        let next = self.setlist.position() + 1;
        let key = ViewKey::of(&last);
        if self
            .preloaded
            .as_ref()
            .is_some_and(|page| page.song == next && page.key == key)
        {
            return false;
        }
        let Some(app) = self.setlist.ready_mut(next) else {
            return false;
        };
        let request = PageRequest {
            song: next,
            offset: 0.0,
            page: last.page.map(|_| 0),
            ..last
        };
        self.preloaded = Some(retain(
            &mut self.compositor,
            &mut self.next_slab_id,
            app,
            &request,
            0,
        ));
        true
    }
}

struct PageReady;

impl Viewer {
    fn request_page(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.generation += 1;
        let _ = self.requests.send(PageRequest {
            generation: self.generation,
            song: self.song,
            width: size.width,
            height: size.height,
            scale: window.scale_factor() as f32,
            inset: TITLE_STRIP_PT,
            offset: self.scroll_offset,
            page: self.paged.then_some(self.page),
            polarity: self.polarity,
            tabs: self.tabs,
            max_texture_side: self.max_texture_side,
        });
    }

    fn accept_ready_page(&mut self) {
        while let Ok(page) = self.results.try_recv() {
            if page.generation != self.generation {
                continue;
            }
            self.scroll_max = page.max_offset;
            self.page_count = page.pages;
            self.page_tabs = page.tabs;
            self.page = self.page.min(page.pages.saturating_sub(1));
            // Leaving paged mode resumes scrolling where the page was.
            if self.paged {
                self.scroll_offset = page.view_offset;
            }
            self.scroll_offset = self.scroll_offset.min(self.scroll_max);
            if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
                if self.uploaded_slab != Some(page.slab.id) {
                    gpu.upload_page(&page.slab.image);
                    self.uploaded_slab = Some(page.slab.id);
                }
                if let Some(status) = &page.status {
                    gpu.upload_status(status);
                }
                gpu.set_view(page.view);
                if self.timing {
                    eprintln!(
                        "accept generation {} @ {} ms",
                        page.generation,
                        since_start()
                    );
                }
                window.set_title(&if self.song_count > 1 {
                    format!(
                        "{}  ·  {} / {}  —  chordpro-live",
                        page.title,
                        page.song + 1,
                        self.song_count
                    )
                } else {
                    format!("{}  —  chordpro-live", page.title)
                });
                window.request_redraw();
            }
        }
    }

    /// Turns to `page`, clamped to the song; no request when it would not move.
    fn turn_page(&mut self, page: usize) {
        let page = page.min(self.page_count.saturating_sub(1));
        if page != self.page {
            self.page = page;
            self.request_page();
        }
    }

    /// Moves through the setlist without wrapping. The page shown stays the
    /// old song's until the new song's complete page arrives.
    fn change_song(&mut self, delta: isize) {
        let target = (self.song as isize + delta).clamp(0, self.song_count as isize - 1) as usize;
        if target == self.song {
            return;
        }
        self.song = target;
        self.scroll_offset = 0.0;
        self.scroll_max = 0.0;
        self.page = 0;
        self.request_page();
    }
}
impl ApplicationHandler<PageReady> for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("chordpro-live")
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.initial_size.0,
                self.initial_size.1,
            ));
        // Chromeless, as switchblade and abner do it: the titlebar is glass
        // with its title hidden and the content view runs under it, so the
        // page's own paper fills the strip and only the traffic lights float
        // over it. No shadow: on macOS 26 the window contour is drawn with
        // the shadow, and dropping the shadow is the only way to lose it.
        #[cfg(target_os = "macos")]
        let attributes = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attributes
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_fullsize_content_view(true)
                .with_has_shadow(false)
        };
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let gpu =
            block_on(Gpu::new(instance, surface, size.width, size.height)).expect("initialize GPU");
        self.max_texture_side = gpu.max_texture_side;
        if self.timing {
            eprintln!(
                "window {}×{} @ {} ms",
                size.width,
                size.height,
                since_start()
            );
        }
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.request_page();
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.window.as_ref().is_none_or(|window| window.id() != id) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.request_page();
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.request_page(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Escape) => event_loop.exit(),
                Key::Character(key) if key.eq_ignore_ascii_case("q") => event_loop.exit(),
                Key::Character(key) if key.eq_ignore_ascii_case("i") => {
                    self.polarity = if self.polarity == Polarity::Light {
                        Polarity::Dark
                    } else {
                        Polarity::Light
                    };
                    self.request_page();
                }
                Key::Character(key) if key.eq_ignore_ascii_case("t") => {
                    // Flip what the performer sees; a song without
                    // tablature leaves the choice alone.
                    if let Some(folded) = self.page_tabs {
                        self.tabs = if folded { Tabs::Show } else { Tabs::Fold };
                        self.request_page();
                    }
                }
                Key::Character(key) if key.eq_ignore_ascii_case("n") => self.change_song(1),
                Key::Character(key) if key.eq_ignore_ascii_case("p") => self.change_song(-1),
                Key::Named(NamedKey::ArrowRight) => self.change_song(1),
                Key::Named(NamedKey::ArrowLeft) => self.change_song(-1),
                Key::Character(key) if key.eq_ignore_ascii_case("v") => {
                    self.paged = !self.paged;
                    if self.paged {
                        // Open on the page holding what is on screen now.
                        self.page = 0;
                    }
                    self.request_page();
                }
                Key::Named(NamedKey::ArrowDown | NamedKey::PageDown | NamedKey::Space)
                    if self.paged =>
                {
                    self.turn_page(self.page + 1)
                }
                Key::Named(NamedKey::ArrowUp | NamedKey::PageUp) if self.paged => {
                    self.turn_page(self.page.saturating_sub(1))
                }
                Key::Named(NamedKey::Home) if self.paged => self.turn_page(0),
                Key::Named(NamedKey::End) if self.paged => self.turn_page(usize::MAX),
                Key::Named(NamedKey::Space) => {
                    let height = self.window.as_ref().unwrap().inner_size().height as f32
                        / self.window.as_ref().unwrap().scale_factor() as f32;
                    self.scroll_offset = (self.scroll_offset + height * 0.8).min(self.scroll_max);
                    self.request_page();
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.scroll_offset = (self.scroll_offset
                        + self.config.line_height * self.config.body_size)
                        .min(self.scroll_max);
                    self.request_page();
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.scroll_offset = (self.scroll_offset
                        - self.config.line_height * self.config.body_size)
                        .max(0.0);
                    self.request_page();
                }
                Key::Named(NamedKey::PageDown) => {
                    let height = self.window.as_ref().unwrap().inner_size().height as f32
                        / self.window.as_ref().unwrap().scale_factor() as f32;
                    self.scroll_offset = (self.scroll_offset + height * 0.8).min(self.scroll_max);
                    self.request_page();
                }
                Key::Named(NamedKey::PageUp) => {
                    let height = self.window.as_ref().unwrap().inner_size().height as f32
                        / self.window.as_ref().unwrap().scale_factor() as f32;
                    self.scroll_offset = (self.scroll_offset - height * 0.8).max(0.0);
                    self.request_page();
                }
                Key::Named(NamedKey::Home) => {
                    self.scroll_offset = 0.0;
                    self.request_page();
                }
                Key::Named(NamedKey::End) => {
                    self.scroll_offset = self.scroll_max;
                    self.request_page();
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let Some(gpu) = &self.gpu else {
                    return;
                };
                let presented = gpu.render();
                if self.timing {
                    eprintln!("present {presented:?} @ {} ms", since_start());
                }
                match presented {
                    Presented::Yes => self.needs_present = false,
                    Presented::Skipped(_) => self.needs_present = true,
                    Presented::Reconfigure => {
                        self.needs_present = true;
                        let size = self.window.as_ref().unwrap().inner_size();
                        self.gpu.as_mut().unwrap().resize(size.width, size.height);
                        self.request_page();
                    }
                }
            }
            // The window became visible: show whatever an occluded present skipped.
            WindowEvent::Occluded(false) if self.needs_present => {
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => {}
        }
    }

    /// While a present is owed, wake on the idle tick to retry it; otherwise
    /// sleep until an event. A truly hidden window costs ten wakes a second
    /// only until it is shown.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(if self.needs_present {
            ControlFlow::WaitUntil(Instant::now() + PRESENT_RETRY)
        } else {
            ControlFlow::Wait
        });
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. })
            && self.needs_present
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: PageReady) {
        self.accept_ready_page();
    }
}

const USAGE: &str = "\
usage: cpl --view <song.cho>...           open songs in the viewer, in order
       cpl --view <folder>                open every ChordPro file in a folder
       cpl --view page-dark <song.cho>... open white on black (`i` toggles)
       cpl --view [page|page-dark|page-inline|page-scroll]
                                          open a built-in sample page
       cpl --view <song.cho> --size 980x720
                                          set the window size in logical points
       cpl --view <folder> --song 2       open on the second song of the setlist
       cpl --view <song.cho> --scroll 300 open scrolled 300 points down the song
       cpl --view <song.cho> --paged      open in paged mode (`v` toggles)
       cpl --view <song.cho> --page 2     open paged on the second page
       cpl --view <song.cho> --tabs fold  open with tablature folded (or show)
       cpl <song.cho>                     print the parsed lyrics
       cpl render <song.cho|sample> --out page.png [--size 980x720]
           [--scale 1|2] [--dark] [--scroll P | --page N] [--tabs show|fold]
                                          write what the window would show
In the viewer: i polarity, v scroll or paged, t fold or show tablature,
n / p or ← → next and previous song, ↑ ↓ / Space / Page Up / Page Down / Home / End scroll or turn
pages, q or Escape closes.";

const SONG_EXTENSIONS: [&str; 6] = ["cho", "chopro", "chordpro", "crd", "pro", "cp"];

/// Each argument is a song file, or a folder whose ChordPro files are taken
/// in name order. A folder with no songs is an error, not an empty setlist.
fn expand_paths(args: &[String]) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for arg in args {
        let path = Path::new(arg);
        if !path.is_dir() {
            paths.push(arg.clone());
            continue;
        }
        let mut found: Vec<String> = fs::read_dir(path)
            .map_err(|error| format!("{arg}: {error}"))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            SONG_EXTENSIONS
                                .iter()
                                .any(|known| known.eq_ignore_ascii_case(ext))
                        })
            })
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        if found.is_empty() {
            return Err(format!("{arg}: no ChordPro files in this folder"));
        }
        found.sort();
        paths.extend(found);
    }
    Ok(paths)
}

const SAMPLES: [&str; 4] = ["page", "page-dark", "page-inline", "page-scroll"];

fn parse_tabs(value: &str) -> Option<Tabs> {
    match value {
        "show" => Some(Tabs::Show),
        "fold" => Some(Tabs::Fold),
        "auto" => Some(Tabs::Auto),
        _ => None,
    }
}

fn parse_size(value: &str) -> Option<(f64, f64)> {
    let (width, height) = value.split_once('x')?;
    let width = width.parse::<f64>().ok()?;
    let height = height.parse::<f64>().ok()?;
    if width.is_finite() && height.is_finite() && width >= 200.0 && height >= 200.0 {
        Some((width, height))
    } else {
        None
    }
}

/// What the window would present for `page`, as RGBA: the fragment shader's
/// placement rule run on the CPU over the same slab and status strip.
fn present(page: &ReadyPage, width: u32, height: u32) -> Vec<u8> {
    let view = page.view;
    let paper = [view.paper, view.paper, view.paper, 255];
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    let slab = &page.slab.image;
    for y in 0..height as i32 {
        let content_y = y - view.inset;
        for x in 0..width as i32 {
            let pixel = |image: &PageImage, row: i32| -> [u8; 4] {
                if row < 0 || x >= image.width as i32 || row >= image.height as i32 {
                    return paper;
                }
                let at = (row as usize * image.width as usize + x as usize) * 4;
                image.pixels[at..at + 4].try_into().unwrap()
            };
            let rgba = if content_y < 0 {
                paper
            } else if content_y >= view.status_top {
                match &page.status {
                    Some(status) => pixel(status, content_y - view.status_top),
                    None => paper,
                }
            } else if content_y < view.band_top || content_y >= view.band_bottom {
                paper
            } else {
                pixel(slab, content_y + view.scroll)
            };
            out.extend_from_slice(&rgba);
        }
    }
    out
}

/// `cpl render`: one song through the worker's real page path, presented as
/// the window would draw it, written to a PNG at any display scale. This is
/// how a 1x page is checked on a 2x Mac, and what visual regression compares.
fn render_main(mut args: Vec<String>) -> ExitCode {
    let mut out = None;
    let mut size = (980.0_f64, 720.0_f64);
    let mut scale = 2.0_f32;
    let mut dark = false;
    let mut offset = 0.0_f32;
    let mut page = None;
    let mut tabs = Tabs::Auto;
    let mut source = None;
    while !args.is_empty() {
        let arg = args.remove(0);
        let mut value = || {
            if args.is_empty() {
                None
            } else {
                Some(args.remove(0))
            }
        };
        let ok = match arg.as_str() {
            "--out" => value().map(|v| out = Some(v)).is_some(),
            "--size" => value()
                .and_then(|v| parse_size(&v))
                .map(|v| size = v)
                .is_some(),
            "--scale" => value()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| (1.0..=4.0).contains(v))
                .map(|v| scale = v)
                .is_some(),
            "--scroll" => value()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
                .map(|v| offset = v)
                .is_some(),
            "--page" => value()
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|v| *v >= 1)
                .map(|v| page = Some(v - 1))
                .is_some(),
            "--tabs" => value()
                .and_then(|v| parse_tabs(&v))
                .map(|v| tabs = v)
                .is_some(),
            "--dark" => {
                dark = true;
                true
            }
            _ if source.is_none() && !arg.starts_with('-') => {
                source = Some(arg.clone());
                true
            }
            _ => false,
        };
        if !ok {
            eprintln!("cpl render: bad argument {arg:?}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    let (Some(source), Some(out)) = (source, out) else {
        eprintln!("cpl render needs a song (or sample name) and --out\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let text = match source.as_str() {
        "page" | "page-dark" => SAMPLE.to_owned(),
        "page-inline" => INLINE_SAMPLE.to_owned(),
        "page-scroll" => SCROLL_SAMPLE.to_owned(),
        path => match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        },
    };
    let dark = dark || source == "page-dark";
    let polarity = if dark {
        Polarity::Dark
    } else {
        Polarity::Light
    };
    let config = Config::default();
    let mut worker = Worker {
        setlist: Setlist::new(vec![cpl_song::parse(&text).song], config.clone(), polarity),
        compositor: PageCompositor::default(),
        config,
        retained: None,
        preloaded: None,
        last: None,
        next_slab_id: 0,
        timing: false,
    };
    let width = (size.0 * scale as f64).round() as u32;
    let height = (size.1 * scale as f64).round() as u32;
    let request = PageRequest {
        generation: 1,
        song: 0,
        width,
        height,
        scale,
        inset: TITLE_STRIP_PT,
        offset,
        page,
        polarity,
        tabs,
        max_texture_side: 8192,
    };
    let (ready, _) = worker.answer(&request);
    let png = cpl_render::png::encode(width, height, &present(&ready, width, height));
    if let Err(error) = fs::write(&out, png) {
        eprintln!("{out}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|arg| arg == "render") {
        return render_main(args.split_off(1));
    }
    // `--paged` anywhere opens in paged mode; trailing `--size WxH`,
    // `--song N`, `--scroll P` and `--page N` options come in any order.
    let mut paged = false;
    args.retain(|arg| {
        let flag = arg == "--paged";
        paged |= flag;
        !flag
    });
    let mut requested_size = None;
    let mut start_song = 0;
    let mut start_offset = 0.0f32;
    let mut start_page = 0;
    let mut tabs = None;
    while args.len() >= 2
        && matches!(
            args[args.len() - 2].as_str(),
            "--size" | "--song" | "--scroll" | "--page" | "--tabs"
        )
    {
        let value = args.pop().unwrap();
        let flag = args.pop().unwrap();
        if flag == "--tabs" {
            match parse_tabs(&value) {
                Some(choice) => tabs = Some(choice),
                None => {
                    eprintln!("invalid --tabs {value:?}; expected show, fold or auto");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--size" {
            match parse_size(&value) {
                Some(size) => requested_size = Some(size),
                None => {
                    eprintln!("invalid --size {value:?}; expected WIDTHxHEIGHT (at least 200x200)");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--page" {
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => {
                    start_page = n - 1;
                    paged = true;
                }
                _ => {
                    eprintln!("invalid --page {value:?}; expected a 1-based page number");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--scroll" {
            // Points down the song, so a capture can reach a scrolled state.
            match value.parse::<f32>() {
                Ok(offset) if offset.is_finite() && offset >= 0.0 => start_offset = offset,
                _ => {
                    eprintln!("invalid --scroll {value:?}; expected points, 0 or more");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            // 1-based, so a capture can open on any song without a keypress.
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => start_song = n - 1,
                _ => {
                    eprintln!("invalid --song {value:?}; expected a 1-based song number");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    // (open a window, sample page name, song paths)
    let (view, sample, paths): (bool, &str, Vec<String>) = match args.as_slice() {
        [flag] if flag == "--view" => (true, "page", Vec::new()),
        [flag, name, rest @ ..] if flag == "--view" && SAMPLES.contains(&name.as_str()) => {
            if !rest.is_empty() && !matches!(name.as_str(), "page" | "page-dark") {
                eprintln!("{USAGE}");
                return ExitCode::FAILURE;
            }
            (true, name.as_str(), rest.to_vec())
        }
        [flag, rest @ ..] if flag == "--view" => (true, "page", rest.to_vec()),
        [path] if !path.starts_with('-') => (false, "page", vec![path.clone()]),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if (requested_size.is_some() || start_song > 0 || start_offset > 0.0 || paged || tabs.is_some())
        && !view
    {
        eprintln!("--size, --song, --scroll, --page, --paged and --tabs require --view");
        return ExitCode::FAILURE;
    }
    let paths = match expand_paths(&paths) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let dark = sample == "page-dark";
    let inline = paths.is_empty() && sample == "page-inline";
    let scroll = paths.is_empty() && sample == "page-scroll";
    // Every song is parsed before the window opens; a bad file fails the
    // launch rather than surprising the performer mid-set.
    let mut songs = Vec::new();
    let mut clean = true;
    if paths.is_empty() {
        let source = if scroll {
            SCROLL_SAMPLE
        } else if inline {
            INLINE_SAMPLE
        } else {
            SAMPLE
        };
        songs.push(cpl_song::parse(source).song);
    }
    for path in &paths {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        };
        let result = cpl_song::parse(&source);
        for diagnostic in &result.diagnostics {
            if paths.len() > 1 {
                eprintln!("{path}: line {}: {}", diagnostic.line, diagnostic.message);
            } else {
                eprintln!("line {}: {}", diagnostic.line, diagnostic.message);
            }
        }
        clean &= result.diagnostics.is_empty();
        songs.push(result.song);
    }
    if !view {
        let song = &songs[0];
        println!("{}", song.first_meta("title").unwrap_or("Untitled"));
        for section in &song.sections {
            println!("\n{}", section.label);
            for line in &section.lines {
                println!("{}", line.lyrics());
            }
        }
        return if clean {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let event_loop = EventLoop::<PageReady>::with_user_event()
        .build()
        .expect("create event loop");
    let (requests, request_rx) = mpsc::channel::<PageRequest>();
    let (result_tx, results) = mpsc::channel::<ReadyPage>();
    let proxy = event_loop.create_proxy();
    let config = Config::default();
    let polarity = if dark {
        Polarity::Dark
    } else {
        Polarity::Light
    };
    let worker_config = config.clone();
    let song_count = songs.len();
    if start_song >= song_count {
        eprintln!(
            "--song {}: the setlist has {song_count} song(s)",
            start_song + 1
        );
        return ExitCode::FAILURE;
    }
    let timing = env::var_os("CPL_TIMING").is_some();
    since_start();
    std::thread::spawn(move || {
        let mut worker = Worker {
            setlist: Setlist::new(songs, worker_config.clone(), polarity),
            compositor: PageCompositor::default(),
            config: worker_config,
            retained: None,
            preloaded: None,
            last: None,
            next_slab_id: 0,
            timing,
        };
        loop {
            let mut request = match request_rx.try_recv() {
                Ok(request) => request,
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    // Idle: prepare the neighbours one unit at a time, checking
                    // for a request between units, then block.
                    if worker.preload_step() {
                        continue;
                    }
                    match request_rx.recv() {
                        Ok(request) => request,
                        Err(_) => break,
                    }
                }
            };
            // Resize and scroll events can arrive faster than a page composes.
            // Only begin the latest pending request.
            while let Ok(newer) = request_rx.try_recv() {
                request = newer;
            }
            let started = Instant::now();
            let (page, composed_slab) = worker.answer(&request);
            if worker.timing {
                eprintln!(
                    "{} {:?} @ {} ms",
                    if composed_slab { "slab" } else { "view" },
                    started.elapsed(),
                    since_start()
                );
            }
            if result_tx.send(page).is_err() || proxy.send_event(PageReady).is_err() {
                break;
            }
            worker.last = Some(request);
        }
    });
    let mut viewer = Viewer {
        timing,
        needs_present: false,
        config,
        polarity,
        tabs: tabs.unwrap_or_default(),
        page_tabs: None,
        initial_size: requested_size.unwrap_or(if inline {
            // 200 pt of page under the 28 pt title strip: tight enough
            // that the sample needs ladder step 9, inline chords.
            (300.0, 228.0)
        } else if scroll {
            (560.0, 340.0)
        } else {
            (980.0, 720.0)
        }),
        scroll_offset: start_offset,
        scroll_max: f32::INFINITY,
        paged,
        page: start_page,
        page_count: usize::MAX,
        song: start_song,
        song_count,
        generation: 0,
        uploaded_slab: None,
        max_texture_side: 8192,
        requests,
        results,
        window: None,
        gpu: None,
    };
    event_loop.run_app(&mut viewer).expect("run viewer");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{expand_paths, parse_size};

    #[test]
    fn a_folder_expands_to_its_songs_in_name_order_and_files_pass_through() {
        let dir = std::env::temp_dir().join(format!("cpl-setlist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.cho", "a.ChoPro", "notes.txt", "c.crd"] {
            std::fs::write(dir.join(name), "{title: x}").unwrap();
        }
        let dir_arg = dir.to_string_lossy().into_owned();
        let paths = expand_paths(&["z.cho".to_owned(), dir_arg.clone()]).unwrap();
        let names: Vec<String> = paths
            .iter()
            .map(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["z.cho", "a.ChoPro", "b.cho", "c.crd"]);
        let empty = dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(expand_paths(&[empty.to_string_lossy().into_owned()]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn window_size_requires_finite_reasonable_dimensions() {
        assert_eq!(parse_size("980x720"), Some((980.0, 720.0)));
        for value in ["980", "0x720", "199x720", "980xNaN", "980xinf", "980x720x2"] {
            assert_eq!(parse_size(value), None, "{value}");
        }
    }
}
