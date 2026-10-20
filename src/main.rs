use cpl_app::{App, Config, Polarity};
use cpl_render::{CoreText, Gpu};
use std::{env, fs, process::ExitCode, sync::Arc, task::Waker};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

const SAMPLE: &str = "{title: The Long Road Home}\n{artist: Sample song}\n{start_of_verse: Verse 1}\n[C]Morning light is [G]falling on the [Am]road\n[F]Every step is [C]bringing us back [G]home\n{end_of_verse}\n{start_of_chorus: Chorus}\n[F]Carry on, [C]carry on\n[G]We can hear the [Am]morning song\n{end_of_chorus}\n{start_of_verse: Verse 2}\n[C]When the night is [G]quiet we will [Am]know\n[F]All the roads we [C]travel lead us [G]home\n{end_of_verse}";

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
    app: App,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
}
impl Viewer {
    fn update_page(&mut self) {
        let (Some(window), Some(gpu)) = (&self.window, &self.gpu) else {
            return;
        };
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let frame = self.app.frame(
            size.width as f32 / scale,
            size.height as f32 / scale,
            &CoreText,
        );
        gpu.upload(&frame, scale);
        window.request_redraw();
    }
}
impl ApplicationHandler for Viewer {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("chordpro-live")
                        .with_inner_size(winit::dpi::LogicalSize::new(980.0, 720.0)),
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
        self.update_page();
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
                self.update_page();
            }
            WindowEvent::ScaleFactorChanged { .. } => self.update_page(),
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
                    self.app.polarity = if self.app.polarity == Polarity::Light {
                        Polarity::Dark
                    } else {
                        Polarity::Light
                    };
                    self.update_page();
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &self.gpu
                    && !gpu.render()
                {
                    let size = self.window.as_ref().unwrap().inner_size();
                    self.gpu.as_mut().unwrap().resize(size.width, size.height);
                    self.update_page();
                }
            }
            _ => {}
        }
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let first = args.next();
    let (view, path) = match first.as_deref() {
        Some("--view") => (true, args.next()),
        path => (false, path.map(str::to_owned)),
    };
    if args.next().is_some() || (!view && path.is_none()) {
        eprintln!("usage: cpl [--view [page|page-dark]] <song.cho>");
        return ExitCode::FAILURE;
    }
    let dark = path.as_deref() == Some("page-dark");
    let source =
        if view && (path.is_none() || matches!(path.as_deref(), Some("page" | "page-dark"))) {
            SAMPLE.to_owned()
        } else {
            let path = path.as_deref().unwrap();
            match fs::read_to_string(path) {
                Ok(source) => source,
                Err(error) => {
                    eprintln!("{path}: {error}");
                    return ExitCode::FAILURE;
                }
            }
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
    let event_loop = EventLoop::new().expect("create event loop");
    let mut viewer = Viewer {
        app: App {
            song: result.song,
            config: Config::default(),
            polarity: if dark {
                Polarity::Dark
            } else {
                Polarity::Light
            },
        },
        window: None,
        gpu: None,
    };
    event_loop.run_app(&mut viewer).expect("run viewer");
    ExitCode::SUCCESS
}
