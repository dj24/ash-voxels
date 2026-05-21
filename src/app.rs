use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::PhysicalKey,
    window::Window,
};

use crate::{assets::VoxelModel, ecs, render::Renderer, vk::AppError};

const DEFAULT_CAPTURE_SIZE: [u32; 2] = [1280, 720];

pub fn run() -> Result<(), AppError> {
    init_tracing();

    match RuntimeConfig::from_env()?.launch_mode {
        LaunchMode::Interactive => run_interactive(),
        LaunchMode::HeadlessCapture { output_path, delay } => run_headless(&output_path, delay),
    }
}

fn run_interactive() -> Result<(), AppError> {
    let event_loop = EventLoop::new().map_err(|error| AppError::Message(error.to_string()))?;
    let mut app = WinitApp { state: None };
    event_loop
        .run_app(&mut app)
        .map_err(|error| AppError::Message(error.to_string()))
}

fn run_headless(output_path: &Path, delay: Duration) -> Result<(), AppError> {
    let mut app = HeadlessAppState::new()?;
    let capture_started_at = Instant::now();

    loop {
        app.render_frame(Instant::now())?;
        if capture_started_at.elapsed() >= delay {
            break;
        }
    }

    app.renderer.save_headless_png(output_path)
}

fn init_tracing() {
    let _ = fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,winit=warn")),
        )
        .with_target(false)
        .try_init();
}

#[derive(Debug, PartialEq, Eq)]
struct RuntimeConfig {
    launch_mode: LaunchMode,
}

#[derive(Debug, PartialEq, Eq)]
enum LaunchMode {
    Interactive,
    HeadlessCapture { output_path: PathBuf, delay: Duration },
}

impl RuntimeConfig {
    fn from_env() -> Result<Self, AppError> {
        parse_runtime_config(std::env::args().skip(1))
    }
}

fn parse_runtime_config<I, S>(args: I) -> Result<RuntimeConfig, AppError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut output_path = None;
    let mut delay = None;
    let mut args = args.into_iter().map(Into::into);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless-png" => {
                let Some(path) = args.next() else {
                    return Err(AppError::Message(
                        "expected a file path after --headless-png".to_string(),
                    ));
                };
                output_path = Some(PathBuf::from(path));
            }
            "--delay-ms" => {
                let Some(raw_delay) = args.next() else {
                    return Err(AppError::Message(
                        "expected a millisecond value after --delay-ms".to_string(),
                    ));
                };
                let parsed = raw_delay.parse::<u64>().map_err(|_| {
                    AppError::Message(format!(
                        "invalid --delay-ms value {raw_delay:?}; expected a non-negative integer"
                    ))
                })?;
                delay = Some(Duration::from_millis(parsed));
            }
            _ => {
                return Err(AppError::Message(format!(
                    "unrecognized launch argument {arg:?}"
                )));
            }
        }
    }

    match (output_path, delay) {
        (None, None) => Ok(RuntimeConfig {
            launch_mode: LaunchMode::Interactive,
        }),
        (Some(output_path), Some(delay)) => Ok(RuntimeConfig {
            launch_mode: LaunchMode::HeadlessCapture { output_path, delay },
        }),
        (Some(_), None) => Err(AppError::Message(
            "--delay-ms is required when using --headless-png".to_string(),
        )),
        (None, Some(_)) => Err(AppError::Message(
            "--headless-png is required when using --delay-ms".to_string(),
        )),
    }
}

struct AppState {
    window: Window,
    world: bevy_ecs::world::World,
    schedule: bevy_ecs::schedule::Schedule,
    renderer: Renderer,
}

impl AppState {
    fn new(event_loop: &ActiveEventLoop) -> Result<Self, AppError> {
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("ash-voxels")
                    .with_inner_size(LogicalSize::new(1280.0, 720.0)),
            )
            .map_err(|error| AppError::Message(error.to_string()))?;

        let initial_size = window.inner_size();
        let voxel_model = VoxelModel::procedural_terrain_chunk();
        let mut world = ecs::create_world([initial_size.width, initial_size.height], &voxel_model);
        let schedule = ecs::create_schedule();

        let renderer = Renderer::new(
            &window,
            window
                .display_handle()
                .map_err(|error| AppError::Message(error.to_string()))?
                .as_raw(),
            window
                .window_handle()
                .map_err(|error| AppError::Message(error.to_string()))?
                .as_raw(),
            [initial_size.width, initial_size.height],
            &voxel_model,
        )?;

        info!("Using GPU {}", renderer.device_caps().device_name);
        world.insert_resource(renderer.device_caps().clone());

        Ok(Self {
            window,
            world,
            schedule,
            renderer,
        })
    }

    fn render_frame(&mut self) -> Result<(), AppError> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }

        ecs::begin_frame(&mut self.world, Instant::now(), size.width, size.height);
        self.schedule.run(&mut self.world);
        let extracted = self
            .world
            .resource::<crate::scene::ExtractedScene>()
            .clone();
        self.renderer.render(&extracted)
    }
}

struct HeadlessAppState {
    world: bevy_ecs::world::World,
    schedule: bevy_ecs::schedule::Schedule,
    renderer: Renderer,
}

impl HeadlessAppState {
    fn new() -> Result<Self, AppError> {
        let voxel_model = VoxelModel::procedural_terrain_chunk();
        let mut world = ecs::create_world(DEFAULT_CAPTURE_SIZE, &voxel_model);
        let schedule = ecs::create_schedule();
        let renderer = Renderer::new_headless(DEFAULT_CAPTURE_SIZE, &voxel_model)?;

        info!("Using GPU {}", renderer.device_caps().device_name);
        world.insert_resource(renderer.device_caps().clone());

        Ok(Self {
            world,
            schedule,
            renderer,
        })
    }

    fn render_frame(&mut self, now: Instant) -> Result<(), AppError> {
        ecs::begin_frame(
            &mut self.world,
            now,
            DEFAULT_CAPTURE_SIZE[0],
            DEFAULT_CAPTURE_SIZE[1],
        );
        self.schedule.run(&mut self.world);
        let extracted = self
            .world
            .resource::<crate::scene::ExtractedScene>()
            .clone();
        self.renderer.render_headless(&extracted)
    }
}

struct WinitApp {
    state: Option<AppState>,
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.state.is_none() {
            match AppState::new(event_loop) {
                Ok(state) => self.state = Some(state),
                Err(error) => {
                    error!("{error}");
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(app_state) = self.state.as_mut() else {
            return;
        };
        if window_id != app_state.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    app_state
                        .world
                        .resource_mut::<ecs::InputState>()
                        .set_key(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::Resized(size) => {
                if let Err(error) = app_state.renderer.handle_resize(size.width, size.height) {
                    error!("{error}");
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = app_state.render_frame() {
                    error!("{error}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app_state) = self.state.as_ref() {
            app_state.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app_state) = self.state.as_mut() {
            if let Err(error) = app_state.renderer.wait_idle() {
                error!("{error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use super::{LaunchMode, RuntimeConfig, parse_runtime_config};

    #[test]
    fn interactive_mode_is_default() {
        let config = parse_runtime_config(Vec::<String>::new()).expect("default launch config");

        assert_eq!(
            config,
            RuntimeConfig {
                launch_mode: LaunchMode::Interactive,
            }
        );
    }

    #[test]
    fn headless_mode_accepts_path_and_delay() {
        let config = parse_runtime_config([
            "--headless-png",
            "capture.png",
            "--delay-ms",
            "1500",
        ])
        .expect("headless launch config");

        assert_eq!(
            config,
            RuntimeConfig {
                launch_mode: LaunchMode::HeadlessCapture {
                    output_path: PathBuf::from("capture.png"),
                    delay: Duration::from_millis(1500),
                },
            }
        );
    }

    #[test]
    fn headless_mode_requires_delay() {
        let error = parse_runtime_config(["--headless-png", "capture.png"])
            .expect_err("missing delay should error");

        assert!(error
            .to_string()
            .contains("--delay-ms is required when using --headless-png"));
    }

    #[test]
    fn negative_delay_is_rejected() {
        let error = parse_runtime_config(["--headless-png", "capture.png", "--delay-ms", "-1"])
            .expect_err("negative delay should error");

        assert!(error.to_string().contains("invalid --delay-ms value"));
    }
}
