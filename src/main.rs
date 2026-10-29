use cpl_app::{
    App, Config, Frame, Library, Polarity, Search, SearchAction, SearchInput, Setlist, Store, Tabs,
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
    /// Keys go to search rather than the page.
    searching: bool,
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
    /// Bumped whenever search replaces the setlist.
    setlist_id: u64,
    /// The file behind each setlist song, when it came from one.
    setlist_paths: Vec<Option<PathBuf>>,
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
    /// Note in the sidecar when each song is shown (the viewer does; `cpl
    /// render` and tests do not).
    record_opens: bool,
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
                searching: false,
                setlist: self.setlist_id,
                song_count: self.setlist.len(),
                slab: Arc::clone(&retained.slab),
                status,
                view,
            },
            composed_slab,
        )
    }

    /// Opens search: rescans the library (reusing unchanged entries), loads
    /// the sidecar the first time, selects the current song, and types
    /// `query`.
    fn open_search(&mut self, query: &str) {
        let library = Library::scan(&self.folders, self.library.as_ref());
        self.load_store();
        let notice = self.store_error.clone();
        let mut search = Search::new(&library, &self.store);
        if let Some(Some(current)) = self.setlist_paths.get(self.setlist.position())
            && let Some(entry) = library.entries.iter().position(|e| &e.path == current)
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
        if !self.record_opens || self.search.is_some() || self.shown == Some(now) {
            return;
        }
        self.shown = Some(now);
        let Some(Some(path)) = self.setlist_paths.get(now.1) else {
            return;
        };
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let entry = cpl_app::Entry::from_source(path, &text);
        self.load_store();
        let Some(store_path) = self.store_path.clone() else {
            return;
        };
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.store
            .set_opened(&entry.fingerprint, &entry.title, when);
        if let Err(error) = self.store.save(&store_path)
            && self.timing
        {
            eprintln!("library store not saved: {error}");
        }
    }

    /// Applies one search key: ratings and tags go to the sidecar, an open
    /// replaces the setlist with the rows, in their order.
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
                let mut paths = Vec::new();
                let mut at = 0;
                for (row, entry) in entries.iter().enumerate() {
                    let path = &library.entries[*entry].path;
                    // A file that vanished since the scan is skipped.
                    let Ok(text) = fs::read_to_string(path) else {
                        continue;
                    };
                    if row <= position {
                        at = songs.len();
                    }
                    songs.push(cpl_song::parse(&text).song);
                    paths.push(Some(path.clone()));
                }
                if songs.is_empty() {
                    search.notice = Some("that song can no longer be read".into());
                    return;
                }
                self.setlist = Setlist::new(songs, self.config.clone(), Polarity::Light);
                self.setlist.go_to(at);
                self.setlist_paths = paths;
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
        let page_px = ((height * request.scale).round() as u32).max(1);
        let slab = compose_slab(
            &mut self.compositor,
            &mut self.next_slab_id,
            &frame,
            page_px,
            request,
            0,
        );
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

/// Where the sidecar lives: `$CPL_LIBRARY_STORE`, else the app's folder in
/// Application Support.
fn default_store_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CPL_LIBRARY_STORE") {
        return Some(PathBuf::from(path));
    }
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/chordpro-live/library.toml"))
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
            inset: TITLE_STRIP_PT,
            offset: self.scroll_offset,
            page: self.paged.then_some(self.page),
            polarity: self.polarity,
            tabs: self.tabs,
            setlist: self.setlist,
            max_texture_side: self.max_texture_side,
        }));
    }

    fn accept_ready_page(&mut self) {
        while let Ok(page) = self.results.try_recv() {
            if page.generation != self.generation {
                continue;
            }
            self.searching = page.searching;
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
            } if self.searching => {
                let input = match logical_key {
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
                Key::Character(key) if key == "/" || key.eq_ignore_ascii_case("o") => {
                    self.searching = true;
                    let _ = self.requests.send(ToWorker::OpenSearch(String::new()));
                    self.request_page();
                }
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
       cpl --view <folder> --search ris   open in search with a query typed
       cpl --view <folder> --search ris --open 2 [--next 1]
                                          open the second result as Enter would,
                                          then step through the results as n does
       cpl --view <folder> --store lib.toml
                                          keep ratings and tags in another sidecar
       cpl <song.cho>                     print the parsed lyrics
       cpl render <song.cho|sample> --out page.png [--size 980x720]
           [--scale 1|2] [--dark] [--scroll P | --page N] [--tabs show|fold]
                                          write what the window would show
In the viewer: i polarity, v scroll or paged, t fold or show tablature,
n / p or ← → next and previous song, ↑ ↓ / Space / Page Up / Page Down / Home / End scroll or turn
pages, q or Escape closes.";

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
    let mut search = None;
    let mut store = None;
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
            "--search" => value().map(|v| search = Some(v)).is_some(),
            "--store" => value().map(|v| store = Some(PathBuf::from(v))).is_some(),
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
    let config = Config::default();
    let sample = SAMPLES.contains(&source.as_str());
    let mut worker = Worker {
        setlist: Setlist::new(vec![cpl_song::parse(&text).song], config.clone(), polarity),
        setlist_id: 0,
        setlist_paths: vec![(!sample).then(|| PathBuf::from(&song_path))],
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
        record_opens: false,
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
        tabs,
        setlist: 0,
        max_texture_side: 8192,
    };
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
    let mut start_search = None;
    let mut open_row = None;
    let mut next_step = 0isize;
    let mut store_path = None;
    while args.len() >= 2
        && matches!(
            args[args.len() - 2].as_str(),
            "--size"
                | "--song"
                | "--scroll"
                | "--page"
                | "--tabs"
                | "--search"
                | "--store"
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
        } else if flag == "--store" {
            store_path = Some(PathBuf::from(value));
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
    if (requested_size.is_some()
        || start_song > 0
        || start_offset > 0.0
        || paged
        || tabs.is_some()
        || start_search.is_some())
        && !view
    {
        eprintln!("--size, --song, --scroll, --page, --paged, --tabs and --search require --view");
        return ExitCode::FAILURE;
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
    let (requests, request_rx) = mpsc::channel::<ToWorker>();
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
    let setlist_paths: Vec<Option<PathBuf>> = if paths.is_empty() {
        vec![None]
    } else {
        paths.iter().map(|path| Some(PathBuf::from(path))).collect()
    };
    let searching = start_search.is_some();
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
            setlist_paths,
            folders,
            library: None,
            store: Store::default(),
            store_path: store_path.or_else(default_store_path),
            store_loaded: false,
            store_error: None,
            search: None,
            record_opens: true,
            shown: None,
            compositor: PageCompositor::default(),
            config: worker_config,
            retained: None,
            preloaded: None,
            last: None,
            next_slab_id: 0,
            timing,
        };
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
        needs_present: false,
        config,
        polarity,
        tabs: tabs.unwrap_or_default(),
        page_tabs: None,
        searching,
        setlist: 0,
        pending_step: next_step,
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
    use super::{
        CoreText, PageCompositor, PathBuf, Polarity, SearchInput, Setlist, Store, Worker,
        expand_paths, library_folders, parse_size,
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
            setlist_paths: vec![Some(PathBuf::from("testdata/greensleeves.cho"))],
            folders: library_folders(&["testdata".to_owned()]),
            library: None,
            store: Store::default(),
            store_path: Some(store),
            store_loaded: false,
            store_error: None,
            search: None,
            record_opens: false,
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
        worker.record_opens = true;
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
