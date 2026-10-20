use cpl_app::{Config, Polarity, Setlist};
use cpl_render::{CoreText, Gpu, PageCompositor, PageImage};
use std::{
    env, fs,
    path::Path,
    process::ExitCode,
    sync::{Arc, mpsc},
    task::Waker,
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
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

struct Viewer {
    config: Config,
    polarity: Polarity,
    initial_size: (f64, f64),
    scroll_offset: f32,
    scroll_max: f32,
    /// Index into the setlist; `n`/`p` move it, the worker swaps the page.
    song: usize,
    song_count: usize,
    generation: u64,
    requests: mpsc::Sender<PageRequest>,
    results: mpsc::Receiver<ReadyPage>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
}

#[derive(Clone, Copy)]
struct PageRequest {
    generation: u64,
    song: usize,
    width: u32,
    height: u32,
    scale: f32,
    offset: f32,
    polarity: Polarity,
}

struct ReadyPage {
    generation: u64,
    song: usize,
    title: String,
    max_offset: f32,
    image: PageImage,
}

/// What a composed page depends on besides the song and the scroll offset.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ViewKey {
    width: u32,
    height: u32,
    scale: u32,
    polarity: Polarity,
}

impl ViewKey {
    fn of(request: &PageRequest) -> Self {
        Self {
            width: request.width,
            height: request.height,
            scale: request.scale.to_bits(),
            polarity: request.polarity,
        }
    }
}

/// The next song's first page, composed while the worker was idle so a song
/// change can be answered without measuring or rasterising anything.
struct Preloaded {
    song: usize,
    key: ViewKey,
    max_offset: f32,
    image: PageImage,
}

/// Measures a missing neighbour or composes the next song's first page at the
/// last viewport. One unit of work per call, so a request never waits behind
/// more than one of them. Returns false once nothing is left to prepare.
fn preload_step(
    setlist: &mut Setlist,
    compositor: &mut PageCompositor,
    last: Option<&PageRequest>,
    preloaded: &mut Option<Preloaded>,
) -> bool {
    if setlist.preload(&CoreText) {
        return true;
    }
    let Some(last) = last else {
        return false;
    };
    let next = setlist.position() + 1;
    let key = ViewKey::of(last);
    if preloaded
        .as_ref()
        .is_some_and(|page| page.song == next && page.key == key)
    {
        return false;
    }
    let Some(app) = setlist.ready_mut(next) else {
        return false;
    };
    app.polarity = last.polarity;
    let (frame, max_offset) = app.frame_scrolled(
        last.width as f32 / last.scale,
        last.height as f32 / last.scale,
        0.0,
    );
    let image = compositor.compose(&frame, last.scale, last.width, last.height);
    *preloaded = Some(Preloaded {
        song: next,
        key,
        max_offset,
        image,
    });
    true
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
            offset: self.scroll_offset,
            polarity: self.polarity,
        });
    }

    fn accept_ready_page(&mut self) {
        while let Ok(page) = self.results.try_recv() {
            if page.generation != self.generation {
                continue;
            }
            self.scroll_max = page.max_offset;
            self.scroll_offset = self.scroll_offset.min(self.scroll_max);
            if let (Some(gpu), Some(window)) = (&mut self.gpu, &self.window) {
                gpu.upload_image(&page.image);
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
        self.request_page();
    }
}
impl ApplicationHandler<PageReady> for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("chordpro-live")
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            self.initial_size.0,
                            self.initial_size.1,
                        )),
                )
                .expect("create window"),
        );
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let gpu =
            block_on(Gpu::new(instance, surface, size.width, size.height)).expect("initialize GPU");
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
                Key::Character(key) if key.eq_ignore_ascii_case("i") => {
                    self.polarity = if self.polarity == Polarity::Light {
                        Polarity::Dark
                    } else {
                        Polarity::Light
                    };
                    self.request_page();
                }
                Key::Character(key) if key.eq_ignore_ascii_case("n") => self.change_song(1),
                Key::Character(key) if key.eq_ignore_ascii_case("p") => self.change_song(-1),
                Key::Named(NamedKey::ArrowRight) => self.change_song(1),
                Key::Named(NamedKey::ArrowLeft) => self.change_song(-1),
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
                if let Some(gpu) = &self.gpu
                    && !gpu.render()
                {
                    let size = self.window.as_ref().unwrap().inner_size();
                    self.gpu.as_mut().unwrap().resize(size.width, size.height);
                    self.request_page();
                }
            }
            _ => {}
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
       cpl <song.cho>                     print the parsed lyrics
In the viewer: i polarity, n / p or ← → next and previous song,
↑ ↓ / Page Up / Page Down / Home / End scroll, Escape closes.";

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

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    // Trailing `--size WxH` and `--song N` options, in either order.
    let mut requested_size = None;
    let mut start_song = 0;
    while args.len() >= 2 && matches!(args[args.len() - 2].as_str(), "--size" | "--song") {
        let value = args.pop().unwrap();
        let flag = args.pop().unwrap();
        if flag == "--size" {
            match parse_size(&value) {
                Some(size) => requested_size = Some(size),
                None => {
                    eprintln!("invalid --size {value:?}; expected WIDTHxHEIGHT (at least 200x200)");
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
    if (requested_size.is_some() || start_song > 0) && !view {
        eprintln!("--size and --song require --view");
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
    std::thread::spawn(move || {
        let mut setlist = Setlist::new(songs, worker_config, polarity);
        let mut compositor = PageCompositor::default();
        // The newest viewport, so the next song is composed at the size it
        // will be shown; and that composition, if it has been done.
        let mut last: Option<PageRequest> = None;
        let mut preloaded: Option<Preloaded> = None;
        loop {
            let mut request = match request_rx.try_recv() {
                Ok(request) => request,
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {
                    // Idle: prepare the neighbours one unit at a time, checking
                    // for a request between units, then block.
                    if preload_step(&mut setlist, &mut compositor, last.as_ref(), &mut preloaded) {
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
            if request.song != setlist.position() {
                setlist.go_to(request.song);
            }
            let key = ViewKey::of(&request);
            let ready = preloaded
                .take_if(|page| {
                    page.song == request.song && page.key == key && request.offset == 0.0
                })
                .map(|page| (page.image, page.max_offset));
            let (image, max_offset) = ready.unwrap_or_else(|| {
                let app = setlist.current(&CoreText);
                app.polarity = request.polarity;
                let (frame, max_offset) = app.frame_scrolled(
                    request.width as f32 / request.scale,
                    request.height as f32 / request.scale,
                    request.offset,
                );
                let image =
                    compositor.compose(&frame, request.scale, request.width, request.height);
                (image, max_offset)
            });
            if result_tx
                .send(ReadyPage {
                    generation: request.generation,
                    song: request.song,
                    title: setlist.title(request.song).to_owned(),
                    max_offset,
                    image,
                })
                .is_err()
                || proxy.send_event(PageReady).is_err()
            {
                break;
            }
            last = Some(request);
        }
    });
    let mut viewer = Viewer {
        config,
        polarity,
        initial_size: requested_size.unwrap_or(if inline {
            (300.0, 200.0)
        } else if scroll {
            (560.0, 340.0)
        } else {
            (980.0, 720.0)
        }),
        scroll_offset: 0.0,
        scroll_max: 0.0,
        song: start_song,
        song_count,
        generation: 0,
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
