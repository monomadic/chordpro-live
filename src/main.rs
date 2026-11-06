mod conform;
mod editor;
mod json;
mod theme_menu;

use cpl_app::{
    Accent, App, Config, Frame, Library, Polarity, Rect, RowsArea, Search, SearchAction,
    SearchInput, Setlist, Store, THEMES, Tabs,
};
use cpl_render::{CoreText, Gpu, PageCompositor, PageImage, Presented, View};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{Arc, mpsc},
    task::Waker,
    time::Instant,
};
use winit::{
    application::ApplicationHandler,
    event::StartCause,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{Window, WindowId},
};

const SAMPLE: &str = "{title: The Long Road Home}\n{artist: Sample song}\n{start_of_verse: Verse 1}\n[C]Morning light is [G]falling on the [Am]road\n[F]Every step is [C]bringing us back [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 2}\n[C]When the night is [G]quiet we will [Am]know\n[F]All the roads we [C]travel lead us [G]home\n{end_of_verse}";
const INLINE_SAMPLE: &str =
    "{title: Test}\n{start_of_verse: A}\n[C]Alpha\n[G]Bravo\n[Am]Charlie\n{end_of_verse}";
const SCROLL_SAMPLE: &str = "{title: The Long Road Home}\n{artist: Sample song}\n{start_of_verse: Verse 1}\n[C]Morning light is [G]falling on the [Am]road\n[F]Every step is [C]bringing us back [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 2}\n[C]When the night is [G]quiet we will [Am]know\n[F]All the roads we [C]travel lead us [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 3}\n[C]Past the station, [G]down beside the [Am]stream\n[F]Through the valley, [C]where the lanterns [G]gleam\n[C]Take the footpath [G]round the maple [Am]tree\n[F]Leave the gate wide [C]open there for [G]me\n{end_of_verse}\n{start_of_verse: Verse 4}\n[C]Hear the river [G]underneath the [Am]bridge\n[F]See the rainclouds [C]moving past the [G]ridge\n[C]When the evening [G]settles on the [Am]field\n[F]We will follow [C]where the road is [G]sealed\n{end_of_verse}\n{start_of_verse: Verse 5}\n[C]One more mile and [G]then the road will [Am]bend\n[F]We can see the [C]village at the [G]end\n[C]There the lights will [G]guide us through the [Am]rain\n[F]To the place we'll [C]call our home a[G]gain\n{end_of_verse}";

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
/// over paper and the page starts below them. Zero in fullscreen.
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
    /// The held modifier keys, for ⌘ and Ctrl shortcuts.
    modifiers: ModifiersState,
    /// `--fullscreen`: open in fake fullscreen, as `f` toggles it.
    start_fullscreen: bool,
    /// The last present was skipped (the window was occluded or the surface
    /// timed out), so the page on the GPU has not been shown. A page that
    /// arrives while a new window is still occluded otherwise stays unseen
    /// until the next keypress.
    needs_present: bool,
    config: Config,
    polarity: Polarity,
    /// The Theme menu's accent, an index into `THEMES`.
    accent: usize,
    /// Wakes the event loop when a Theme menu item is chosen.
    menu_proxy: Option<winit::event_loop::EventLoopProxy<PageReady>>,
    /// The tablature choice; `t` unfolds a folded tab or folds a shown one.
    tabs: Tabs,
    /// Whether the accepted page has tablature, and whether it is folded.
    page_tabs: Option<bool>,
    /// The accepted page's tablature labels, which a click toggles, and
    /// whether the pointer shows the hand over one.
    toggles: Vec<Rect>,
    hand: bool,
    /// Keys go to search rather than the page.
    searching: bool,
    /// Search's result rows as last drawn, and the pointer in page points.
    rows: Option<RowsArea>,
    pointer: Option<(f32, f32)>,
    /// Opened with no songs: the library is the home screen, and closing it
    /// before a song is opened quits rather than showing a placeholder.
    library_home: bool,
    /// The worker's setlist this viewer is in; search may replace it.
    setlist: u64,
    /// `--next N`: songs to step, as `n`/`p` would, once search has opened
    /// a setlist, so a capture can reach that state without a keypress.
    pending_step: isize,
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
    requests: mpsc::Sender<ToWorker>,
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
    /// The Theme menu's choice, an index into `THEMES`.
    accent: usize,
    tabs: Tabs,
    /// The setlist the host thinks it is in; an open from search makes a
    /// new one, and `song` then means nothing until the host catches up.
    setlist: u64,
    max_texture_side: u32,
}

/// What the window thread sends the worker. Page requests may be coalesced
/// to the newest; search keys never are, so none is lost while typing.
enum ToWorker {
    Page(PageRequest),
    /// Open search over the library, with this query typed.
    OpenSearch(String),
    Search(SearchInput),
    /// Open the file belonging to the displayed page in the external editor.
    Edit {
        setlist: u64,
        song: usize,
    },
    /// ⌘V, ⌘C and ⌘X in search: the worker reads or writes the clipboard,
    /// so the window thread never waits on it.
    Clipboard(Clipboard),
    /// Shift a song's chords: `[` `]` and `0` in the viewer.
    Transpose {
        setlist: u64,
        song: usize,
        shift: Shift,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clipboard {
    Paste,
    Copy,
    Cut,
}

#[derive(Clone, Copy, Debug)]
enum Shift {
    By(i32),
    Reset,
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
    /// The worker is showing search, not a song.
    searching: bool,
    /// Where search drew its result rows, for the pointer.
    rows: Option<RowsArea>,
    /// The tablature labels that show, in window points below the title
    /// strip: a click on one folds or shows the tab.
    toggles: Vec<Rect>,
    setlist: u64,
    song_count: usize,
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
    accent: usize,
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
            accent: request.accent,
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

/// A setlist song's file and its sidecar key, read once when the setlist is
/// made so the sidecar never needs the file again.
struct SongFile {
    path: PathBuf,
    fingerprint: String,
    title: String,
}

impl SongFile {
    fn new(path: &Path, text: &str) -> Self {
        let entry = cpl_app::Entry::from_source(path, text);
        Self {
            path: entry.path,
            fingerprint: entry.fingerprint,
            title: entry.title,
        }
    }
}

struct Worker {
    setlist: Setlist,
    /// Bumped whenever search replaces the setlist.
    setlist_id: u64,
    /// The file behind each setlist song, when it came from one.
    setlist_files: Vec<Option<SongFile>>,
    /// Folders search lists; scanned when search opens.
    folders: Vec<PathBuf>,
    library: Option<Library>,
    store: Store,
    /// Where the sidecar lives; `None` once it failed to load, so a file
    /// this build cannot read is never overwritten.
    store_path: Option<PathBuf>,
    store_loaded: bool,
    /// Why the sidecar is read-only, shown each time search opens.
    store_error: Option<String>,
    search: Option<Search>,
    /// Write to the sidecar when a song is shown or transposed (the viewer
    /// does; `chordpro-live render` and tests do not).
    writes_store: bool,
    /// The setlist and song last noted as shown.
    shown: Option<(u64, usize)>,
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
    compositor.accent = THEMES[request.accent];
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
        self.compositor.accent = THEMES[request.accent];
        if self.search.is_some() {
            return (self.answer_search(request), true);
        }
        // A request from before search replaced the setlist opens the new
        // setlist where search put it, at the top of the song.
        let mut request = *request;
        if request.setlist != self.setlist_id {
            request.song = self.setlist.position();
            request.offset = 0.0;
            request.page = request.page.map(|_| 0);
            request.setlist = self.setlist_id;
        }
        let request = &request;
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
        let status = (!scroll.status.is_empty()).then(|| {
            let c = &self.config;
            let strip = c.margin_bottom + c.status_height * c.scroll_size();
            let strip_px = (strip * scale).ceil() as u32;
            view.status_top = request.content_px() as i32 - strip_px as i32;
            let mut text = scroll.status;
            for item in &mut text {
                item.baseline -= height - strip;
            }
            let frame = Frame {
                width,
                height: strip,
                polarity: request.polarity,
                text,
                shapes: Vec::new(),
                fits: false,
                columns: 1,
                ladder_step: 10,
                toggles: Vec::new(),
                clip: None,
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
                searching: false,
                rows: None,
                toggles: retained
                    .page
                    .toggles
                    .iter()
                    .map(|rect| Rect {
                        y: rect.y - scroll.offset,
                        ..*rect
                    })
                    .filter(|rect| {
                        rect.y >= scroll.band.0 - 1e-3
                            && rect.y + rect.height <= scroll.band.1 + 1e-3
                    })
                    .collect(),
                setlist: self.setlist_id,
                song_count: self.setlist.len(),
                slab: Arc::clone(&retained.slab),
                status,
                view,
            },
            composed_slab,
        )
    }

    fn edit(&self, setlist: u64, song: usize) {
        if self.search.is_some() || setlist != self.setlist_id {
            return;
        }
        let Some(Some(file)) = self.setlist_files.get(song) else {
            return; // Built-in samples have no source file.
        };
        let path = file.path.clone();
        std::thread::spawn(move || {
            if let Err(error) = editor::open(&path) {
                eprintln!("could not open {} in $EDITOR: {error}", path.display());
            }
        });
    }

    /// Transposes a song of the setlist the host is showing. Its page is
    /// measured and composed again on the next request; the old page stays
    /// on screen until then.
    fn transpose(&mut self, setlist: u64, song: usize, shift: Shift) {
        if self.search.is_some() || setlist != self.setlist_id || song >= self.setlist.len() {
            return;
        }
        let semitones = match shift {
            Shift::By(delta) => self.setlist.transpose(song) + delta,
            Shift::Reset => 0,
        };
        if self.setlist.set_transpose(song, semitones) {
            self.retained.take_if(|r| r.song == song);
            self.preloaded.take_if(|p| p.song == song);
            let semitones = self.setlist.transpose(song);
            self.write_store(song, |store, file| {
                store.set_transpose(&file.fingerprint, &file.title, semitones)
            });
        }
    }

    /// Opens search: rescans the library (reusing unchanged entries), loads
    /// the sidecar the first time, selects the current song, and types
    /// `query`.
    fn open_search(&mut self, query: &str) {
        let library = Library::scan(&self.folders, self.library.as_ref());
        self.load_store();
        let notice = self.store_error.clone();
        let mut search = Search::new(&library, &self.store);
        if let Some(Some(current)) = self.setlist_files.get(self.setlist.position())
            && let Some(entry) = library
                .entries
                .iter()
                .position(|e| e.path == current.path && e.fingerprint == current.fingerprint)
                .or_else(|| library.entries.iter().position(|e| e.path == current.path))
        {
            search.select(entry);
        }
        for c in query.chars() {
            search.input(SearchInput::Char(c), &library, &self.store);
        }
        search.notice = notice;
        self.library = Some(library);
        self.search = Some(search);
    }

    /// While search is open: rescans the library folders, reusing every
    /// unchanged entry. Returns whether what search lists changed, so the
    /// caller redraws only then.
    fn rescan(&mut self) -> bool {
        let (Some(search), Some(old)) = (self.search.as_mut(), self.library.as_ref()) else {
            return false;
        };
        let library = Library::scan(&self.folders, Some(old));
        if library.entries == old.entries {
            return false;
        }
        search.rescanned(old, &library, &self.store);
        self.library = Some(library);
        true
    }

    /// Loads the sidecar the first time it is needed. A file that cannot be
    /// read leaves the store read-only for the session, never overwritten,
    /// and search says why.
    fn load_store(&mut self) {
        if self.store_loaded {
            return;
        }
        self.store_loaded = true;
        let Some(path) = &self.store_path else {
            return;
        };
        match Store::load(path) {
            Ok(store) => self.store = store,
            Err(error) => {
                self.store_path = None;
                self.store_error = Some(format!("tags and ratings are read-only: {error}"));
            }
        }
    }

    /// Notes in the sidecar that the current song is being shown, once per
    /// song change. Runs after the page has gone to the host, so the read
    /// and write never delay it; a sample with no file is not noted.
    fn note_shown(&mut self) {
        let now = (self.setlist_id, self.setlist.position());
        if !self.writes_store || self.search.is_some() || self.shown == Some(now) {
            return;
        }
        self.shown = Some(now);
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.write_store(now.1, |store, file| {
            store.set_opened(&file.fingerprint, &file.title, when)
        });
    }

    /// Changes the sidecar record of setlist song `song` and saves it. A
    /// sample with no file, a read-only sidecar, or `chordpro-live render` writes
    /// nothing.
    fn write_store(&mut self, song: usize, change: impl FnOnce(&mut Store, &SongFile)) {
        if !self.writes_store || !matches!(self.setlist_files.get(song), Some(Some(_))) {
            return;
        }
        self.load_store();
        let Some(Some(file)) = self.setlist_files.get(song) else {
            return;
        };
        let Some(store_path) = &self.store_path else {
            return;
        };
        change(&mut self.store, file);
        if let Err(error) = self.store.save(store_path)
            && self.timing
        {
            eprintln!("library store not saved: {error}");
        }
    }

    /// Opens each song of a new setlist at the transpose it was last played
    /// at. Reads the sidecar the first time; nothing is measured here.
    fn restore_transposes(&mut self) {
        self.load_store();
        for (song, file) in self.setlist_files.iter().enumerate() {
            if let Some(file) = file {
                self.setlist
                    .set_transpose(song, self.store.transpose(&file.fingerprint));
            }
        }
    }

    /// Applies one search key: ratings and tags go to the sidecar, an open
    /// replaces the setlist with the rows, in their order.
    /// Pastes the clipboard into the query, or copies the query to it (and,
    /// for cut, empties the query). The clipboard is `pbpaste`/`pbcopy`, run
    /// here on the worker.
    fn clipboard(&mut self, action: Clipboard) {
        let (Some(search), Some(library)) = (self.search.as_mut(), self.library.as_ref()) else {
            return;
        };
        if action == Clipboard::Paste {
            if let Ok(out) = std::process::Command::new("pbpaste").output() {
                search.insert_text(&String::from_utf8_lossy(&out.stdout), library, &self.store);
            }
            return;
        }
        let copied = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .take()
                    .expect("piped stdin")
                    .write_all(search.copied_text().as_bytes())?;
                child.wait()
            })
            .is_ok_and(|status| status.success());
        if action == Clipboard::Cut && copied {
            search.cut(library, &self.store);
        }
    }

    fn search_input(&mut self, input: SearchInput) {
        let (Some(search), Some(library)) = (self.search.as_mut(), self.library.as_ref()) else {
            return;
        };
        match search.input(input, library, &self.store) {
            SearchAction::Redraw => {}
            SearchAction::Close => {
                self.search = None;
                self.retained = None;
            }
            SearchAction::Rate { entry, rating } => {
                let song = &library.entries[entry];
                self.store
                    .set_rating(&song.fingerprint, &song.title, rating);
                let done = if rating == 0 {
                    format!("cleared the rating of {}", song.title)
                } else {
                    format!("rated {} {}", song.title, "★".repeat(rating as usize))
                };
                search.notice = Some(save(&self.store, self.store_path.as_deref(), done));
                search.refilter(library, &self.store);
            }
            SearchAction::Tag { entry, tag } => {
                let song = &library.entries[entry];
                let added = self.store.toggle_tag(&song.fingerprint, &song.title, &tag);
                let done = if added {
                    format!("tagged {} #{tag}", song.title)
                } else if song.file_tags.contains(&tag) {
                    format!("#{tag} is in the file itself; it stays")
                } else {
                    format!("removed #{tag} from {}", song.title)
                };
                search.notice = Some(save(&self.store, self.store_path.as_deref(), done));
                search.refilter(library, &self.store);
            }
            SearchAction::Open { entries, position } => {
                let mut songs = Vec::new();
                let mut files = Vec::new();
                let mut at = 0;
                for (row, entry) in entries.iter().enumerate() {
                    let entry = &library.entries[*entry];
                    // A file that vanished since the scan is skipped, as is
                    // a song its file no longer holds.
                    let Ok(text) = fs::read_to_string(&entry.path) else {
                        continue;
                    };
                    let Some(part) = cpl_song::split(&text).get(entry.part).copied() else {
                        continue;
                    };
                    if row <= position {
                        at = songs.len();
                    }
                    songs.push(cpl_song::parse(part.text).song);
                    files.push(Some(SongFile::new(&entry.path, part.text)));
                }
                if songs.is_empty() {
                    search.notice = Some("that song can no longer be read".into());
                    return;
                }
                self.setlist = Setlist::new(songs, self.config.clone(), Polarity::Light);
                self.setlist.go_to(at);
                self.setlist_files = files;
                self.restore_transposes();
                self.setlist_id += 1;
                self.search = None;
                self.retained = None;
                self.preloaded = None;
            }
        }
    }

    /// The search screen, composed whole: it changes with every key.
    fn answer_search(&mut self, request: &PageRequest) -> ReadyPage {
        let (width, height) = request.logical();
        let started = Instant::now();
        let search = self.search.as_mut().unwrap();
        let library = self.library.as_ref().unwrap();
        let frame = search.frame(
            width,
            height,
            request.polarity,
            library,
            &self.store,
            &self.config,
            &CoreText,
        );
        let laid_out = started.elapsed();
        let page_px = ((height * request.scale).round() as u32).max(1);
        let slab = compose_slab(
            &mut self.compositor,
            &mut self.next_slab_id,
            &frame,
            page_px,
            request,
            0,
        );
        if self.timing {
            eprintln!(
                "search frame {laid_out:?}, raster {:?}, {} texts",
                started.elapsed() - laid_out,
                frame.text.len()
            );
        }
        // The page retained before search is stale once it closes.
        self.retained = None;
        ReadyPage {
            generation: request.generation,
            song: self.setlist.position(),
            title: "Search".into(),
            max_offset: 0.0,
            pages: 1,
            view_offset: 0.0,
            tabs: None,
            searching: true,
            rows: self.search.as_ref().and_then(Search::rows_area),
            toggles: Vec::new(),
            setlist: self.setlist_id,
            song_count: self.setlist.len(),
            slab,
            status: None,
            view: View {
                scroll: 0,
                band_top: 0,
                band_bottom: i32::MAX,
                status_top: i32::MAX,
                inset: request.inset_px() as i32,
                paper: if request.polarity == Polarity::Light {
                    255
                } else {
                    0
                },
            },
        }
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

/// Saves the sidecar after a change and says what happened, or why the
/// change is only in memory.
fn save(store: &Store, path: Option<&Path>, done: String) -> String {
    match path {
        None => format!("{done} (not saved: the library file is read-only)"),
        Some(path) => match store.save(path) {
            Ok(()) => done,
            Err(error) => format!("{done} (not saved: {error})"),
        },
    }
}

/// Fake fullscreen, after switchblade's `toggle_fullscreen` (sb-window):
/// simple fullscreen resizes the window over the desktop instead of moving
/// it to its own Space. Leaving checks both kinds, so one `f` always gets
/// out, whichever put the window there.
fn toggle_fullscreen(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        if window.simple_fullscreen() {
            window.set_simple_fullscreen(false);
        } else if window.fullscreen().is_some() {
            window.set_fullscreen(None);
        } else {
            window.set_simple_fullscreen(true);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let next = if window.fullscreen().is_some() {
            None
        } else {
            Some(winit::window::Fullscreen::Borderless(None))
        };
        window.set_fullscreen(next);
    }
}

fn is_fullscreen(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        window.simple_fullscreen() || window.fullscreen().is_some()
    }
    #[cfg(not(target_os = "macos"))]
    {
        window.fullscreen().is_some()
    }
}

/// Where the sidecar lives: `$CPL_LIBRARY_STORE`, else the app's folder in
/// Application Support.
fn default_store_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CPL_LIBRARY_STORE") {
        return Some(PathBuf::from(path));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/chordpro-live/library.toml"))
}

/// The page config: the bundled defaults, with the user's `config.toml` laid
/// over them when there is one. The file is `--config PATH`, else
/// `$CPL_CONFIG`, else `config.toml` beside the sidecar in Application
/// Support, read only if it exists. A file named on purpose must exist.
/// `--floor` then sets the readability floor over whatever the file says.
fn load_config(path: Option<PathBuf>, floor: Option<f32>) -> Result<Config, String> {
    let named = path.or_else(|| env::var_os("CPL_CONFIG").map(PathBuf::from));
    let path = named.clone().or_else(|| {
        let home = env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/chordpro-live/config.toml"))
    });
    let config = match path {
        Some(path) if named.is_some() || path.exists() => {
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Config::with_overrides(&text).map_err(|error| format!("{}: {error}", path.display()))?
        }
        _ => Config::default(),
    };
    match floor {
        Some(points) => config.with_floor(points),
        None => Ok(config),
    }
}

fn parse_floor(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|v| cpl_app::FLOOR_RANGE.contains(v))
}

/// What a bare launch opens as the library: the config's `[library]
/// folders`, else the folder the app started in, else, started from Finder
/// (in `/`, or nowhere), `[library] app_folder`. A leading `~` is `home`.
fn home_library(config: &Config, current: Option<PathBuf>, home: Option<PathBuf>) -> Vec<PathBuf> {
    let expand = |folder: &str| match (folder.strip_prefix('~'), &home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(folder),
    };
    if !config.library_folders.is_empty() {
        return config.library_folders.iter().map(|f| expand(f)).collect();
    }
    match current {
        Some(current) if current != Path::new("/") => vec![current],
        _ => vec![expand(&config.app_library_folder)],
    }
}

/// The folders search lists: each folder named on the command line, and the
/// folder of each song file named on it.
fn library_folders(args: &[String]) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = Vec::new();
    for arg in args {
        let path = Path::new(arg);
        let folder = if path.is_dir() {
            path.to_path_buf()
        } else {
            match path.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                _ => PathBuf::from("."),
            }
        };
        if !folders.contains(&folder) {
            folders.push(folder);
        }
    }
    folders
}

impl Viewer {
    /// Flips what the performer sees, from `t` or a click on a tab's
    /// label; a song without tablature leaves the choice alone.
    fn toggle_tabs(&mut self) {
        if let Some(folded) = self.page_tabs {
            self.tabs = if folded { Tabs::Show } else { Tabs::Fold };
            self.request_page();
        }
    }

    /// Whether the pointer is on a tablature label of the page shown.
    fn over_toggle(&self) -> bool {
        !self.searching
            && self
                .pointer
                .is_some_and(|(x, y)| self.toggles.iter().any(|rect| rect.contains(x, y)))
    }

    /// A hand over a tab label says it can be clicked.
    fn update_cursor(&mut self) {
        let hand = self.over_toggle();
        if hand != self.hand
            && let Some(window) = &self.window
        {
            self.hand = hand;
            window.set_cursor(if hand {
                winit::window::CursorIcon::Pointer
            } else {
                winit::window::CursorIcon::Default
            });
        }
    }

    fn request_page(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.generation += 1;
        let _ = self.requests.send(ToWorker::Page(PageRequest {
            generation: self.generation,
            song: self.song,
            width: size.width,
            height: size.height,
            scale: window.scale_factor() as f32,
            // Fullscreen has no title bar to keep the page clear of.
            inset: if is_fullscreen(window) {
                0.0
            } else {
                TITLE_STRIP_PT
            },
            offset: self.scroll_offset,
            page: self.paged.then_some(self.page),
            polarity: self.polarity,
            accent: self.accent,
            tabs: self.tabs,
            setlist: self.setlist,
            max_texture_side: self.max_texture_side,
        }));
    }

    fn transpose(&mut self, shift: Shift) {
        let _ = self.requests.send(ToWorker::Transpose {
            setlist: self.setlist,
            song: self.song,
            shift,
        });
        self.request_page();
    }

    fn accept_ready_page(&mut self) {
        while let Ok(page) = self.results.try_recv() {
            if page.generation != self.generation {
                continue;
            }
            self.searching = page.searching;
            self.rows = page.rows;
            self.toggles = page.toggles;
            self.update_cursor();
            if page.setlist != self.setlist {
                // Search opened a new setlist: follow it, from the top.
                self.setlist = page.setlist;
                self.song = page.song;
                self.song_count = page.song_count;
                self.scroll_offset = 0.0;
                self.page = 0;
                let step = std::mem::take(&mut self.pending_step);
                if step != 0 {
                    // The opened song is still drawn below until the stepped
                    // one arrives, exactly as after a keypress.
                    self.change_song(step);
                }
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
                window.set_title(&if page.searching {
                    "Search  —  chordpro-live".to_owned()
                } else if self.song_count > 1 {
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
        if let Some(proxy) = self.menu_proxy.take() {
            theme_menu::install(self.accent, move || {
                let _ = proxy.send_event(PageReady);
            });
        }
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
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            // The pointer in search: moving onto a row focuses it (only a
            // move does, so a resting pointer never steals the keyboard's
            // row), a click opens it, and the wheel scrolls the list.
            WindowEvent::CursorMoved { position, .. } => {
                let window = self.window.as_ref().unwrap();
                let scale = window.scale_factor() as f32;
                let inset = if is_fullscreen(window) {
                    0.0
                } else {
                    TITLE_STRIP_PT
                };
                let point = (position.x as f32 / scale, position.y as f32 / scale - inset);
                self.pointer = Some(point);
                if !self.searching {
                    self.update_cursor();
                    return;
                }
                let row = self.rows.and_then(|area| {
                    area.row_at(point.0, point.1)
                        .filter(|&row| row != area.cursor)
                });
                if let Some(row) = row {
                    let _ = self
                        .requests
                        .send(ToWorker::Search(SearchInput::Focus(row)));
                    self.request_page();
                }
            }
            WindowEvent::CursorLeft { .. } => self.pointer = None,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if self.searching => {
                let row = self
                    .rows
                    .zip(self.pointer)
                    .and_then(|(area, (x, y))| area.row_at(x, y));
                if let Some(row) = row {
                    let _ = self
                        .requests
                        .send(ToWorker::Search(SearchInput::OpenRow(row)));
                    self.request_page();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.over_toggle() {
                    self.toggle_tabs();
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.searching => {
                let Some(area) = self.rows else {
                    return;
                };
                // Wheel notches arrive as lines, a row each; a trackpad as
                // points, which move the list by exactly as much so it
                // follows the fingers. Positive y moves the content down,
                // which is up the list.
                let rows = -match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        let scale = self.window.as_ref().unwrap().scale_factor();
                        (p.y / scale) as f32 / area.row_height
                    }
                };
                if rows != 0.0 {
                    let _ = self
                        .requests
                        .send(ToWorker::Search(SearchInput::Scroll(rows)));
                    self.request_page();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        ..
                    },
                ..
            } if self.searching => {
                let cmd = self.modifiers.super_key();
                let ctrl = self.modifiers.control_key();
                let alt = self.modifiers.alt_key();
                let shift = self.modifiers.shift_key();
                let letter = match &logical_key {
                    Key::Character(text) if cmd || ctrl => text.to_lowercase().chars().next(),
                    _ => None,
                };
                let clipboard = match letter {
                    Some('v') if cmd => Some(Clipboard::Paste),
                    Some('c') if cmd => Some(Clipboard::Copy),
                    Some('x') if cmd => Some(Clipboard::Cut),
                    _ => None,
                };
                if let Some(action) = clipboard {
                    let _ = self.requests.send(ToWorker::Clipboard(action));
                    self.request_page();
                    return;
                }
                if cmd && letter == Some('q') {
                    event_loop.exit();
                    return;
                }
                let input = match logical_key {
                    // Emacs keys, as every macOS text field takes them.
                    Key::Character(_) if ctrl => match letter {
                        Some('a') => Some(SearchInput::LineStart),
                        Some('e') => Some(SearchInput::LineEnd),
                        Some('b') => Some(SearchInput::Left),
                        Some('f') => Some(SearchInput::Right),
                        Some('w') => Some(SearchInput::DeleteWord),
                        Some('u') => Some(SearchInput::DeleteToStart),
                        Some('k') => Some(SearchInput::DeleteToEnd),
                        Some('d') => Some(SearchInput::DeleteForward),
                        Some('h') => Some(SearchInput::Backspace),
                        Some('n') => Some(SearchInput::Down),
                        Some('p') => Some(SearchInput::Up),
                        _ => None,
                    },
                    Key::Character(_) if cmd && letter == Some('a') => Some(SearchInput::SelectAll),
                    // Other ⌘ letters are app shortcuts, never typed text.
                    Key::Character(_) if cmd => None,
                    Key::Named(NamedKey::ArrowLeft) if shift && cmd => {
                        Some(SearchInput::SelectLineStart)
                    }
                    Key::Named(NamedKey::ArrowRight) if shift && cmd => {
                        Some(SearchInput::SelectLineEnd)
                    }
                    Key::Named(NamedKey::ArrowLeft) if shift && alt => {
                        Some(SearchInput::SelectWordLeft)
                    }
                    Key::Named(NamedKey::ArrowRight) if shift && alt => {
                        Some(SearchInput::SelectWordRight)
                    }
                    Key::Named(NamedKey::ArrowLeft) if shift => Some(SearchInput::SelectLeft),
                    Key::Named(NamedKey::ArrowRight) if shift => Some(SearchInput::SelectRight),
                    Key::Named(NamedKey::Backspace) if cmd => Some(SearchInput::DeleteToStart),
                    Key::Named(NamedKey::Backspace) if alt => Some(SearchInput::DeleteWord),
                    Key::Named(NamedKey::ArrowLeft) if cmd => Some(SearchInput::LineStart),
                    Key::Named(NamedKey::ArrowRight) if cmd => Some(SearchInput::LineEnd),
                    Key::Named(NamedKey::ArrowLeft) if alt => Some(SearchInput::WordLeft),
                    Key::Named(NamedKey::ArrowRight) if alt => Some(SearchInput::WordRight),
                    Key::Named(NamedKey::ArrowUp) if cmd => Some(SearchInput::Home),
                    Key::Named(NamedKey::ArrowDown) if cmd => Some(SearchInput::End),
                    Key::Named(NamedKey::ArrowLeft) => Some(SearchInput::Left),
                    Key::Named(NamedKey::ArrowRight) => Some(SearchInput::Right),
                    Key::Named(NamedKey::Delete) => Some(SearchInput::DeleteForward),
                    Key::Named(NamedKey::Escape) => Some(SearchInput::Cancel),
                    Key::Named(NamedKey::Enter) => Some(SearchInput::Enter),
                    Key::Named(NamedKey::Backspace) => Some(SearchInput::Backspace),
                    Key::Named(NamedKey::ArrowUp) => Some(SearchInput::Up),
                    Key::Named(NamedKey::ArrowDown) => Some(SearchInput::Down),
                    Key::Named(NamedKey::PageUp) => Some(SearchInput::PageUp),
                    Key::Named(NamedKey::PageDown) => Some(SearchInput::PageDown),
                    Key::Named(NamedKey::Home) => Some(SearchInput::Home),
                    Key::Named(NamedKey::End) => Some(SearchInput::End),
                    Key::Named(NamedKey::Tab) => Some(SearchInput::Sort),
                    Key::Named(NamedKey::Space) => Some(SearchInput::Char(' ')),
                    Key::Character(text) => {
                        for c in text.chars() {
                            let _ = self.requests.send(ToWorker::Search(SearchInput::Char(c)));
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(input) = input {
                    let _ = self.requests.send(ToWorker::Search(input));
                }
                self.request_page();
            }
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
                // ⌘Q quits and ⌘F finds, as in any Mac app.
                Key::Character(key)
                    if self.modifiers.super_key() && key.eq_ignore_ascii_case("q") =>
                {
                    event_loop.exit()
                }
                Key::Character(key)
                    if key == "/"
                        || key.eq_ignore_ascii_case("o")
                        || self.modifiers.super_key() && key.eq_ignore_ascii_case("f") =>
                {
                    self.searching = true;
                    let _ = self.requests.send(ToWorker::OpenSearch(String::new()));
                    self.request_page();
                }
                Key::Character(key) if key.eq_ignore_ascii_case("q") => event_loop.exit(),
                Key::Character(key) if key.eq_ignore_ascii_case("e") => {
                    let _ = self.requests.send(ToWorker::Edit {
                        setlist: self.setlist,
                        song: self.song,
                    });
                }
                Key::Character(key) if key.eq_ignore_ascii_case("i") => {
                    self.polarity = if self.polarity == Polarity::Light {
                        Polarity::Dark
                    } else {
                        Polarity::Light
                    };
                    self.request_page();
                }
                // `f`: fake fullscreen, as switchblade's `fast_fullscreen`
                // (sb-window `toggle_fullscreen`): the window grows over the
                // desktop with no Space animation. It leaves either mode.
                Key::Character(key)
                    if !self.modifiers.super_key() && key.eq_ignore_ascii_case("f") =>
                {
                    if let Some(window) = &self.window {
                        toggle_fullscreen(window);
                    }
                    self.request_page();
                }
                Key::Character(key) if key.eq_ignore_ascii_case("t") => self.toggle_tabs(),
                Key::Character(key) if key == "]" => self.transpose(Shift::By(1)),
                Key::Character(key) if key == "[" => self.transpose(Shift::By(-1)),
                Key::Character(key) if key == "0" => self.transpose(Shift::Reset),
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
                        + self.config.line_height * self.config.scroll_size())
                    .min(self.scroll_max);
                    self.request_page();
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.scroll_offset = (self.scroll_offset
                        - self.config.line_height * self.config.scroll_size())
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
                    Presented::Yes => {
                        self.needs_present = false;
                        // `--fullscreen` waits for the window's first present:
                        // entering simple fullscreen before the window is up
                        // leaves its surface occluded and the page unseen.
                        if std::mem::take(&mut self.start_fullscreen) {
                            toggle_fullscreen(self.window.as_ref().unwrap());
                            self.request_page();
                        }
                    }
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: PageReady) {
        if let Some(accent) = theme_menu::take_choice() {
            self.accent = accent;
            self.request_page();
        }
        self.accept_ready_page();
        if self.library_home && !self.searching && self.setlist == 0 {
            event_loop.exit();
        }
    }
}

const USAGE: &str = "\
usage: chordpro-live                      open the library of the current folder
       chordpro-live <song.cho>           open a song
       chordpro-live <song.cho> <more.cho>...
                                          open the first song, the rest queued
                                          after it as a setlist (n / p)
       chordpro-live <folder>             open every ChordPro file in a folder
       chordpro-live [page|page-dark|page-inline|page-scroll]
                                          open a built-in sample page
     Press e to open the current file in $EDITOR (or vi), in Terminal.
     Options come after the songs:
       --dark                             open white on black (`i` toggles)
       --size 980x720                     set the window size in logical points
       --song 2                           open on the second song of the setlist
       --scroll 300                       open scrolled 300 points down the song
       --paged                            open in paged mode (`v` toggles)
       --page 2                           open paged on the second page
       --fullscreen                       open in fake fullscreen (`f` toggles)
       --tabs show                        open with tablature shown (fold, the default, or auto)
       --transpose 2                      open transposed up two semitones
       --search ris [--open 2 [--next 1]] open in search with a query typed,
                                          open the second result as Enter would,
                                          then step through them as n does
       --store lib.toml                   keep ratings and tags in another sidecar
       --floor 10                         let body text go down to 10 pt before
                                          the page compacts or scrolls
       --config my.toml                   lay my.toml's [page] and [library]
                                          keys over the defaults (else
                                          $CPL_CONFIG, else config.toml in
                                          Application Support)
       chordpro-live print <song.cho>     print the parsed lyrics
       chordpro-live render <song.cho|sample> --out page.png [--size 980x720]
           [--scale 1|2] [--dark] [--scroll P | --page N] [--tabs show|fold]
           [--transpose N] [--floor PT] [--config my.toml]
                                          write what the window would show
       chordpro-live conform [<song.cho|folder>...] [--table note.md [--check]]
                                          compare cpl-song's parse with the
                                          reference chordpro's, construct by
                                          construct and file by file
In the viewer: f fullscreen, / o or ⌘F search, ⌘Q quits, i polarity, v scroll or paged, t fold or show tablature,
[ ] transpose down or up a semitone, 0 back to the chords as written,
n / p or ← → next and previous song, ↑ ↓ / Space / Page Up / Page Down / Home / End scroll or turn
pages, q or Escape quits.";

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
            .filter(|path| path.is_file() && cpl_app::library::is_song(path))
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

/// `print`: the parsed lyrics of each song in a file, for a quick look at
/// what the parser understood. Fails when the parser reported anything.
fn print_main(args: &[String]) -> ExitCode {
    let [path] = args else {
        eprintln!("chordpro-live print needs one song file\n{USAGE}");
        return ExitCode::FAILURE;
    };
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mut clean = true;
    for (index, part) in cpl_song::split(&source).into_iter().enumerate() {
        let result = cpl_song::parse(part.text);
        for diagnostic in &result.diagnostics {
            eprintln!(
                "line {}: {}",
                diagnostic.line + part.offset,
                diagnostic.message
            );
        }
        clean &= result.diagnostics.is_empty();
        let song = &result.song;
        if index > 0 {
            println!();
        }
        println!("{}", song.first_meta("title").unwrap_or("Untitled"));
        for section in &song.sections {
            println!("\n{}", section.label);
            for line in &section.lines {
                if section.kind == cpl_song::SectionKind::Grid {
                    println!("{}", line.cells().collect::<Vec<_>>().join(" ").trim());
                } else {
                    println!("{}", line.lyrics());
                }
            }
        }
    }
    if clean {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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

/// Semitones for `--transpose`: `2`, `+2` or `-3`, within an octave.
fn parse_transpose(value: &str) -> Option<i32> {
    let value = value.strip_prefix('+').unwrap_or(value);
    value.parse().ok().filter(|n: &i32| (-11..=11).contains(n))
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

/// `chordpro-live render`: one song through the worker's real page path, presented as
/// the window would draw it, written to a PNG at any display scale. This is
/// how a 1x page is checked on a 2x Mac, and what visual regression compares.
fn render_main(mut args: Vec<String>) -> ExitCode {
    let mut out = None;
    let mut size = (980.0_f64, 720.0_f64);
    let mut scale = 2.0_f32;
    let mut dark = false;
    let mut accent = 0;
    let mut offset = 0.0_f32;
    let mut page = None;
    let mut tabs = Tabs::default();
    let mut transpose = 0;
    let mut search = None;
    let mut store = None;
    let mut config_path = None;
    let mut floor = None;
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
            "--transpose" => value()
                .and_then(|v| parse_transpose(&v))
                .map(|v| transpose = v)
                .is_some(),
            "--search" => value().map(|v| search = Some(v)).is_some(),
            "--accent" => value()
                .and_then(|v| Accent::named(&v))
                .map(|v| accent = v)
                .is_some(),
            "--store" => value().map(|v| store = Some(PathBuf::from(v))).is_some(),
            "--config" => value()
                .map(|v| config_path = Some(PathBuf::from(v)))
                .is_some(),
            "--floor" => value()
                .and_then(|v| parse_floor(&v))
                .map(|v| floor = Some(v))
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
            eprintln!("chordpro-live render: bad argument {arg:?}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    }
    let (Some(source), Some(out)) = (source, out) else {
        eprintln!("chordpro-live render needs a song (or sample name) and --out\n{USAGE}");
        return ExitCode::FAILURE;
    };
    // A folder renders its first song, or its search screen.
    let song_path = if Path::new(&source).is_dir() {
        match expand_paths(std::slice::from_ref(&source)) {
            Ok(paths) => paths[0].clone(),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        source.clone()
    };
    let text = match song_path.as_str() {
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
    let config = match load_config(config_path, floor) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let sample = SAMPLES.contains(&source.as_str());
    // A file of several songs renders its first.
    let text = cpl_song::split(&text)[0].text.to_owned();
    let mut worker = Worker {
        setlist: Setlist::new(vec![cpl_song::parse(&text).song], config.clone(), polarity),
        setlist_id: 0,
        setlist_files: vec![(!sample).then(|| SongFile::new(Path::new(&song_path), &text))],
        folders: if sample {
            Vec::new()
        } else {
            library_folders(std::slice::from_ref(&source))
        },
        library: None,
        store: Store::default(),
        store_path: store.or_else(default_store_path),
        store_loaded: false,
        store_error: None,
        search: None,
        writes_store: false,
        shown: None,
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
        accent,
        tabs,
        setlist: 0,
        max_texture_side: 8192,
    };
    worker.restore_transposes();
    worker.transpose(0, 0, Shift::By(transpose));
    if let Some(query) = &search {
        worker.open_search(query);
    }
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
    if args.first().is_some_and(|arg| arg == "conform") {
        return conform::main(args.split_off(1));
    }
    if args.first().is_some_and(|arg| arg == "render") {
        return render_main(args.split_off(1));
    }
    if args.first().is_some_and(|arg| arg == "print") {
        return print_main(&args[1..]);
    }
    // `--paged` anywhere opens in paged mode; trailing `--size WxH`,
    // `--song N`, `--scroll P` and `--page N` options come in any order.
    let mut paged = false;
    let mut start_fullscreen = false;
    let mut dark = false;
    args.retain(|arg| match arg.as_str() {
        "--paged" => {
            paged = true;
            false
        }
        "--fullscreen" => {
            start_fullscreen = true;
            false
        }
        "--dark" => {
            dark = true;
            false
        }
        _ => true,
    });
    let mut requested_size = None;
    let mut start_song = 0;
    let mut start_offset = 0.0f32;
    let mut start_page = 0;
    let mut tabs = None;
    let mut start_transpose = 0;
    let mut start_search = None;
    let mut open_row = None;
    let mut next_step = 0isize;
    let mut store_path = None;
    let mut config_path = None;
    let mut floor = None;
    let mut accent = 0;
    while args.len() >= 2
        && matches!(
            args[args.len() - 2].as_str(),
            "--size"
                | "--song"
                | "--scroll"
                | "--page"
                | "--tabs"
                | "--transpose"
                | "--search"
                | "--store"
                | "--config"
                | "--floor"
                | "--accent"
                | "--open"
                | "--next"
        )
    {
        let value = args.pop().unwrap();
        let flag = args.pop().unwrap();
        if flag == "--search" {
            start_search = Some(value);
        } else if flag == "--open" {
            // 1-based row of the search results, opened as Enter would.
            match value.parse::<usize>() {
                Ok(n) if n >= 1 => open_row = Some(n - 1),
                _ => {
                    eprintln!("invalid --open {value:?}; expected a 1-based row number");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--next" {
            // Songs to step after the open; negative steps back, as `p` does.
            match value.parse::<isize>() {
                Ok(n) => next_step = n,
                _ => {
                    eprintln!("invalid --next {value:?}; expected a whole number of songs");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--accent" {
            match Accent::named(&value) {
                Some(index) => accent = index,
                None => {
                    let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
                    eprintln!("invalid --accent {value:?}; expected {}", names.join(", "));
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--store" {
            store_path = Some(PathBuf::from(value));
        } else if flag == "--config" {
            config_path = Some(PathBuf::from(value));
        } else if flag == "--floor" {
            match parse_floor(&value) {
                Some(points) => floor = Some(points),
                None => {
                    eprintln!("invalid --floor {value:?}; expected points, 6 to 72");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--transpose" {
            match parse_transpose(&value) {
                Some(n) => start_transpose = n,
                None => {
                    eprintln!("invalid --transpose {value:?}; expected semitones, -11 to 11");
                    return ExitCode::FAILURE;
                }
            }
        } else if flag == "--tabs" {
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
    // A sample name that is not a file opens that built-in page; anything
    // else is a song file or a folder. No arguments opens the library.
    let (sample, paths): (&str, Vec<String>) = match args.as_slice() {
        [name] if SAMPLES.contains(&name.as_str()) && !Path::new(name).exists() => {
            (name.as_str(), Vec::new())
        }
        rest if rest.iter().any(|arg| arg.starts_with('-')) => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
        rest => ("page", rest.to_vec()),
    };
    let library_home = args.is_empty();
    if library_home && start_search.is_none() {
        start_search = Some(String::new());
    }
    if (open_row.is_some() && start_search.is_none()) || (next_step != 0 && open_row.is_none()) {
        eprintln!("--open needs --search, and --next needs --open");
        return ExitCode::FAILURE;
    }
    let folders = library_folders(&paths);
    let paths = match expand_paths(&paths) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let dark = dark || sample == "page-dark";
    let inline = paths.is_empty() && sample == "page-inline";
    let scroll = paths.is_empty() && sample == "page-scroll";
    // Every song is parsed before the window opens; a bad file fails the
    // launch rather than surprising the performer mid-set.
    let mut songs = Vec::new();
    let mut setlist_files = Vec::new();
    if paths.is_empty() {
        let source = if scroll {
            SCROLL_SAMPLE
        } else if inline {
            INLINE_SAMPLE
        } else {
            SAMPLE
        };
        songs.push(cpl_song::parse(source).song);
        setlist_files.push(None);
    }
    for path in &paths {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        };
        // A file of several songs adds each to the setlist.
        for part in cpl_song::split(&source) {
            let result = cpl_song::parse(part.text);
            for diagnostic in &result.diagnostics {
                let line = diagnostic.line + part.offset;
                if paths.len() > 1 {
                    eprintln!("{path}: line {line}: {}", diagnostic.message);
                } else {
                    eprintln!("line {line}: {}", diagnostic.message);
                }
            }
            songs.push(result.song);
            setlist_files.push(Some(SongFile::new(Path::new(path), part.text)));
        }
    }
    let event_loop = EventLoop::<PageReady>::with_user_event()
        .build()
        .expect("create event loop");
    let (requests, request_rx) = mpsc::channel::<ToWorker>();
    let (result_tx, results) = mpsc::channel::<ReadyPage>();
    let proxy = event_loop.create_proxy();
    let menu_proxy = event_loop.create_proxy();
    let config = match load_config(config_path, floor) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let folders = if library_home {
        let folders = home_library(
            &config,
            env::current_dir().ok(),
            env::var_os("HOME").map(PathBuf::from),
        );
        // The app's own folder is made on first launch, so there is
        // somewhere obvious to put songs; a folder the config names is not.
        if config.library_folders.is_empty() {
            for folder in &folders {
                let _ = fs::create_dir_all(folder);
            }
        }
        folders
    } else {
        folders
    };
    let polarity = if dark {
        Polarity::Dark
    } else {
        Polarity::Light
    };
    let worker_config = config.clone();
    let song_count = songs.len();
    let searching = start_search.is_some();
    if start_transpose != 0 {
        // As `]` or `[` pressed that many times on the opening song.
        let _ = requests.send(ToWorker::Transpose {
            setlist: 0,
            song: start_song,
            shift: Shift::By(start_transpose),
        });
    }
    if let Some(query) = start_search {
        let _ = requests.send(ToWorker::OpenSearch(query));
        // The same keys a performer would press: to the top row, down to
        // the chosen one, Enter.
        if let Some(row) = open_row {
            let _ = requests.send(ToWorker::Search(SearchInput::Home));
            for _ in 0..row {
                let _ = requests.send(ToWorker::Search(SearchInput::Down));
            }
            let _ = requests.send(ToWorker::Search(SearchInput::Enter));
        }
    }
    if start_song >= song_count {
        eprintln!(
            "--song {}: the setlist has {song_count} song(s)",
            start_song + 1
        );
        return ExitCode::FAILURE;
    }
    let timing = env::var_os("CPL_TIMING").is_some();
    since_start();
    let rescan_every = std::time::Duration::from_secs_f32(config.search_rescan_seconds.max(0.25));
    std::thread::spawn(move || {
        let mut worker = Worker {
            setlist: Setlist::new(songs, worker_config.clone(), polarity),
            setlist_id: 0,
            setlist_files,
            folders,
            library: None,
            store: Store::default(),
            store_path: store_path.or_else(default_store_path),
            store_loaded: false,
            store_error: None,
            search: None,
            writes_store: true,
            shown: None,
            compositor: PageCompositor::default(),
            config: worker_config,
            retained: None,
            preloaded: None,
            last: None,
            next_slab_id: 0,
            timing,
        };
        worker.restore_transposes();
        loop {
            let first = match request_rx.try_recv() {
                Ok(message) => message,
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    // Idle: prepare the neighbours one unit at a time, checking
                    // for a request between units, then block. While search
                    // is open, wake to rescan the library and redraw the
                    // current search screen if a song came, went or changed.
                    if worker.preload_step() {
                        continue;
                    }
                    if worker.search.is_some() {
                        match request_rx.recv_timeout(rescan_every) {
                            Ok(message) => message,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => {
                                if worker.rescan()
                                    && let Some(request) = worker.last
                                {
                                    let (page, _) = worker.answer(&request);
                                    if result_tx.send(page).is_err()
                                        || proxy.send_event(PageReady).is_err()
                                    {
                                        break;
                                    }
                                }
                                continue;
                            }
                        }
                    } else {
                        match request_rx.recv() {
                            Ok(message) => message,
                            Err(_) => break,
                        }
                    }
                }
            };
            // Resize and scroll events can arrive faster than a page composes,
            // so only the latest page request is answered; every search key
            // is applied, in order.
            let mut latest = None;
            for message in
                std::iter::once(first).chain(std::iter::from_fn(|| request_rx.try_recv().ok()))
            {
                match message {
                    ToWorker::Page(request) => latest = Some(request),
                    ToWorker::OpenSearch(query) => worker.open_search(&query),
                    ToWorker::Search(input) => worker.search_input(input),
                    ToWorker::Edit { setlist, song } => worker.edit(setlist, song),
                    ToWorker::Clipboard(action) => worker.clipboard(action),
                    ToWorker::Transpose {
                        setlist,
                        song,
                        shift,
                    } => worker.transpose(setlist, song, shift),
                }
            }
            let Some(request) = latest else {
                continue;
            };
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
            worker.note_shown();
        }
    });
    let mut viewer = Viewer {
        timing,
        modifiers: ModifiersState::empty(),
        start_fullscreen,
        needs_present: false,
        config,
        polarity,
        accent,
        menu_proxy: Some(menu_proxy),
        tabs: tabs.unwrap_or_default(),
        page_tabs: None,
        toggles: Vec::new(),
        hand: false,
        searching,
        rows: None,
        pointer: None,
        library_home,
        setlist: 0,
        pending_step: next_step,
        initial_size: requested_size.unwrap_or(if inline {
            // Keep the title visible while exercising compact chord rows.
            (300.0, 244.0)
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
    use super::{
        CoreText, PageCompositor, PathBuf, Polarity, SearchInput, Setlist, Shift, SongFile, Store,
        Worker, expand_paths, home_library, library_folders, parse_size,
    };
    use cpl_app::Config;

    fn worker(store: PathBuf) -> Worker {
        let config = Config::default();
        let text = std::fs::read_to_string("testdata/greensleeves.cho").unwrap();
        Worker {
            setlist: Setlist::new(
                vec![cpl_song::parse(&text).song],
                config.clone(),
                Polarity::Light,
            ),
            setlist_id: 0,
            setlist_files: vec![Some(SongFile::new(
                std::path::Path::new("testdata/greensleeves.cho"),
                &text,
            ))],
            folders: library_folders(&["testdata".to_owned()]),
            library: None,
            store: Store::default(),
            store_path: Some(store),
            store_loaded: false,
            store_error: None,
            search: None,
            writes_store: false,
            shown: None,
            compositor: PageCompositor::default(),
            config,
            retained: None,
            preloaded: None,
            last: None,
            next_slab_id: 0,
            timing: false,
        }
    }

    #[test]
    fn an_open_search_picks_up_a_song_saved_into_the_folder() {
        let dir = std::env::temp_dir().join(format!("cpl-rescan-{}", std::process::id()));
        let songs = dir.join("songs");
        std::fs::create_dir_all(&songs).unwrap();
        std::fs::copy("testdata/greensleeves.cho", songs.join("greensleeves.cho")).unwrap();
        let mut worker = worker(dir.join("library.toml"));
        worker.folders = vec![songs.clone()];
        assert!(!worker.rescan(), "nothing to rescan before search opens");
        worker.open_search("");
        assert_eq!(worker.search.as_ref().unwrap().results().len(), 1);
        assert!(!worker.rescan(), "an unchanged folder redraws nothing");
        std::fs::write(songs.join("new.cho"), "{title: Brand New}\n[C]Words").unwrap();
        assert!(worker.rescan());
        let search = worker.search.as_ref().unwrap();
        assert_eq!(search.results().len(), 2);
        let library = worker.library.as_ref().unwrap();
        assert_eq!(
            library.entries[search.selected().unwrap()].title,
            "Greensleeves",
            "the selection stays on the song it was on"
        );
        std::fs::remove_file(songs.join("new.cho")).unwrap();
        assert!(worker.rescan());
        assert_eq!(worker.search.as_ref().unwrap().results().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn showing_a_song_notes_when_in_the_sidecar_once_per_change() {
        let dir = std::env::temp_dir().join(format!("cpl-opened-{}", std::process::id()));
        let path = dir.join("library.toml");
        let mut worker = worker(path.clone());
        worker.note_shown();
        assert!(!path.exists(), "the test worker records nothing");
        worker.writes_store = true;
        worker.note_shown();
        let text = std::fs::read_to_string("testdata/greensleeves.cho").unwrap();
        let greensleeves = cpl_app::library::fingerprint(&text);
        let first = Store::load(&path).unwrap().opened(&greensleeves);
        assert!(first > 1_700_000_000, "a current time, not {first}");
        // The same song again writes nothing.
        std::fs::remove_file(&path).unwrap();
        worker.note_shown();
        assert!(!path.exists());
        // Opening another song from search notes that one.
        worker.open_search("rising");
        worker.search_input(SearchInput::Enter);
        worker.note_shown();
        let store = Store::load(&path).unwrap();
        let rising = std::fs::read_to_string("testdata/house_of_the_rising_sun.cho").unwrap();
        assert!(store.opened(&cpl_app::library::fingerprint(&rising)) >= first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bare_launch_opens_the_configured_folders_else_where_it_started() {
        let home = Some(PathBuf::from("/Users/ada"));
        let config = Config::default();
        assert_eq!(
            home_library(&config, Some(PathBuf::from("/Users/ada/gig")), home.clone()),
            [PathBuf::from("/Users/ada/gig")],
            "a terminal launch opens the folder it started in"
        );
        for finder in [Some(PathBuf::from("/")), None] {
            assert_eq!(
                home_library(&config, finder, home.clone()),
                [PathBuf::from("/Users/ada/Music/ChordPro")],
                "a Finder launch starts in / and opens the app's folder"
            );
        }
        let config = Config::with_overrides(
            "[library]\nfolders = [\"~/Songs\", \"/Volumes/Gig # set\"] # mine\napp_folder = \"~\"\n",
        )
        .unwrap();
        assert_eq!(
            home_library(&config, Some(PathBuf::from("/")), home.clone()),
            [
                PathBuf::from("/Users/ada/Songs"),
                PathBuf::from("/Volumes/Gig # set")
            ]
        );
        assert_eq!(config.app_library_folder, "~");
        for bad in [
            "[library]\nfolders = \"~/one\"\n",
            "[library]\napp_folder = [\"~\"]\n",
            "[library]\nfolder = []\n",
            "[library]\nfolders = [~/Songs]\n",
            "[library]\napp_folder = \"open\n",
        ] {
            assert!(Config::with_overrides(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_song_in_a_file_is_its_own_setlist_entry_and_search_row() {
        let dir = std::env::temp_dir().join(format!("cpl-parts-{}", std::process::id()));
        let mut worker = worker(dir.join("library.toml"));
        let path = std::path::Path::new("testdata/harbour_lights.cho");
        let text = std::fs::read_to_string(path).unwrap();
        let parts = cpl_song::split(&text);
        assert_eq!(parts.len(), 2);
        worker.setlist = Setlist::new(
            parts.iter().map(|p| cpl_song::parse(p.text).song).collect(),
            Config::default(),
            Polarity::Light,
        );
        worker.setlist_files = parts
            .iter()
            .map(|p| Some(SongFile::new(path, p.text)))
            .collect();
        worker.setlist.go_to(1);
        // Search opens on the song showing, not on its file's first song.
        worker.open_search("");
        let library = worker.library.as_ref().unwrap();
        let selected = &library.entries[worker.search.as_ref().unwrap().selected().unwrap()];
        assert_eq!(
            (selected.title.as_str(), selected.part),
            ("Harbour Lights (reprise)", 1)
        );
        // Opening it reads that song from the file.
        worker.search_input(SearchInput::Enter);
        let song = worker.setlist.current(&CoreText).song();
        assert_eq!(song.first_meta("title"), Some("Harbour Lights (reprise)"));
    }

    #[test]
    fn a_transpose_is_remembered_and_restored_when_the_song_opens_again() {
        let dir = std::env::temp_dir().join(format!("cpl-transpose-{}", std::process::id()));
        let path = dir.join("library.toml");
        let text = std::fs::read_to_string("testdata/greensleeves.cho").unwrap();
        let greensleeves = cpl_app::library::fingerprint(&text);
        let mut worker = worker(path.clone());
        worker.transpose(0, 0, Shift::By(1));
        assert!(!path.exists(), "the test worker records nothing");
        worker.writes_store = true;
        worker.transpose(0, 0, Shift::By(1));
        worker.transpose(1, 0, Shift::By(5));
        assert_eq!(
            worker.setlist.transpose(0),
            2,
            "a stale setlist's key is dropped"
        );
        assert_eq!(Store::load(&path).unwrap().transpose(&greensleeves), 2);
        // A new session opens the song at +2, its chords already shifted.
        let mut again = super::tests::worker(path.clone());
        again.restore_transposes();
        assert_eq!(again.setlist.transpose(0), 2);
        assert_eq!(
            again.setlist.current(&CoreText).song().first_meta("key"),
            Some("Bm")
        );
        // Opening it from search restores it too.
        let mut from_search = super::tests::worker(dir.join("other.toml"));
        from_search.store_path = Some(path.clone());
        from_search.open_search("greensleeves");
        from_search.search_input(SearchInput::Enter);
        assert_eq!(from_search.setlist.transpose(0), 2);
        // Reset forgets it.
        worker.transpose(0, 0, Shift::Reset);
        assert!(Store::load(&path).unwrap().record(&greensleeves).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_opens_on_the_current_song_and_opening_a_row_makes_the_rows_the_setlist() {
        let dir = std::env::temp_dir().join(format!("cpl-search-{}", std::process::id()));
        let mut worker = worker(dir.join("library.toml"));
        worker.open_search("");
        let search = worker.search.as_ref().unwrap();
        let library = worker.library.as_ref().unwrap();
        let selected = &library.entries[search.selected().unwrap()];
        assert_eq!(
            selected.title, "Greensleeves",
            "the current song is selected"
        );
        // Filter to the two folk-ish titles, move to the second, open it.
        for c in "r".chars() {
            worker.search_input(SearchInput::Char(c));
        }
        let rows = worker.search.as_ref().unwrap().results().len();
        assert!(rows >= 2);
        worker.search_input(SearchInput::Down);
        let second = {
            let search = worker.search.as_ref().unwrap();
            worker.library.as_ref().unwrap().entries[search.selected().unwrap()]
                .title
                .clone()
        };
        worker.search_input(SearchInput::Enter);
        assert!(worker.search.is_none());
        assert_eq!(worker.setlist_id, 1);
        assert_eq!(worker.setlist.len(), rows);
        assert_eq!(worker.setlist.position(), 1);
        assert_eq!(worker.setlist.title(1), second);
        let _ = CoreText;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rating_and_tagging_write_the_sidecar_and_never_a_song() {
        let dir = std::env::temp_dir().join(format!("cpl-rate-{}", std::process::id()));
        let path = dir.join("library.toml");
        let before = std::fs::read_to_string("testdata/greensleeves.cho").unwrap();
        let mut worker = worker(path.clone());
        worker.open_search("");
        worker.search_input(SearchInput::Char('4'));
        for c in "#Encore".chars() {
            worker.search_input(SearchInput::Char(c));
        }
        worker.search_input(SearchInput::Enter);
        let store = Store::load(&path).unwrap();
        let fingerprint = cpl_app::library::fingerprint(&before);
        assert_eq!(store.rating(&fingerprint), 4);
        assert_eq!(store.tags(&fingerprint), ["encore"]);
        let notice = worker.search.as_ref().unwrap().notice.clone().unwrap();
        assert!(notice.contains("tagged Greensleeves #encore"), "{notice}");
        assert_eq!(
            std::fs::read_to_string("testdata/greensleeves.cho").unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_sidecar_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("cpl-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.toml");
        std::fs::write(&path, "this is not a library").unwrap();
        let mut worker = worker(path.clone());
        worker.open_search("");
        assert!(
            worker
                .search
                .as_ref()
                .unwrap()
                .notice
                .as_ref()
                .unwrap()
                .contains("read-only")
        );
        worker.search_input(SearchInput::Char('5'));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "this is not a library"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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
