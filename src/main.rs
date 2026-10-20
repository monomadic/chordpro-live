use cpl_app::{App, Config, Polarity};
use cpl_render::{CoreText, Gpu, PageCompositor, PageImage};
use std::{
    env, fs,
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
    generation: u64,
    requests: mpsc::Sender<PageRequest>,
    results: mpsc::Receiver<ReadyPage>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
}

struct PageRequest {
    generation: u64,
    width: u32,
    height: u32,
    scale: f32,
    offset: f32,
    polarity: Polarity,
}

struct ReadyPage {
    generation: u64,
    max_offset: f32,
    image: PageImage,
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
                window.request_redraw();
            }
        }
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
usage: cpl --view <song.cho>              open a song in the viewer
       cpl --view page-dark <song.cho>    open it white on black (`i` toggles)
       cpl --view [page|page-dark|page-inline|page-scroll]
                                          open a built-in sample page
       cpl --view <song.cho> --size 980x720
                                          set the window size in logical points
       cpl <song.cho>                     print the parsed lyrics
In the viewer: i polarity, arrows / Page Up / Page Down / Home / End scroll,
Escape closes.";

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
    let requested_size = if args.len() >= 2 && args[args.len() - 2] == "--size" {
        let value = args.pop().unwrap();
        args.pop();
        match parse_size(&value) {
            Some(size) => Some(size),
            None => {
                eprintln!("invalid --size {value:?}; expected WIDTHxHEIGHT (at least 200x200)");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    // (open a window, sample page name, song path)
    let (view, sample, path) = match args.as_slice() {
        [flag] if flag == "--view" => (true, "page", None),
        [flag, name] if flag == "--view" && SAMPLES.contains(&name.as_str()) => {
            (true, name.as_str(), None)
        }
        [flag, path] if flag == "--view" => (true, "page", Some(path)),
        [flag, name, path] if flag == "--view" && matches!(name.as_str(), "page" | "page-dark") => {
            (true, name.as_str(), Some(path))
        }
        [path] if !path.starts_with('-') => (false, "page", Some(path)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if requested_size.is_some() && !view {
        eprintln!("--size requires --view");
        return ExitCode::FAILURE;
    }
    let dark = sample == "page-dark";
    let inline = path.is_none() && sample == "page-inline";
    let scroll = path.is_none() && sample == "page-scroll";
    let source = match path {
        Some(path) => match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{path}: {error}");
                return ExitCode::FAILURE;
            }
        },
        None if scroll => SCROLL_SAMPLE.to_owned(),
        None if inline => INLINE_SAMPLE.to_owned(),
        None => SAMPLE.to_owned(),
    };
    let result = cpl_song::parse(&source);
    for diagnostic in &result.diagnostics {
        eprintln!("line {}: {}", diagnostic.line, diagnostic.message);
    }
    if !view {
        println!("{}", result.song.first_meta("title").unwrap_or("Untitled"));
        for section in &result.song.sections {
            println!("\n{}", section.label);
            for line in &section.lines {
                println!("{}", line.lyrics());
            }
        }
        return if result.diagnostics.is_empty() {
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
    std::thread::spawn(move || {
        let mut app = App::new(result.song, worker_config, polarity, &CoreText);
        let mut compositor = PageCompositor::default();
        while let Ok(mut request) = request_rx.recv() {
            // Resize and scroll events can arrive faster than a page composes.
            // Only begin the latest pending request.
            while let Ok(newer) = request_rx.try_recv() {
                request = newer;
            }
            app.polarity = request.polarity;
            let (frame, max_offset) = app.frame_scrolled(
                request.width as f32 / request.scale,
                request.height as f32 / request.scale,
                request.offset,
            );
            let image = compositor.compose(&frame, request.scale, request.width, request.height);
            if result_tx
                .send(ReadyPage {
                    generation: request.generation,
                    max_offset,
                    image,
                })
                .is_err()
                || proxy.send_event(PageReady).is_err()
            {
                break;
            }
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
    use super::parse_size;

    #[test]
    fn window_size_requires_finite_reasonable_dimensions() {
        assert_eq!(parse_size("980x720"), Some((980.0, 720.0)));
        for value in ["980", "0x720", "199x720", "980xNaN", "980xinf", "980x720x2"] {
            assert_eq!(parse_size(value), None, "{value}");
        }
    }
}
