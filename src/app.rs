use std::time::Instant;

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

use crate::{
    ecs,
    render::Renderer,
    vk::AppError,
};

pub fn run() -> Result<(), AppError> {
    init_tracing();

    let event_loop = EventLoop::new().map_err(|error| AppError::Message(error.to_string()))?;
    let mut app = WinitApp { state: None };
    event_loop
        .run_app(&mut app)
        .map_err(|error| AppError::Message(error.to_string()))
}

fn init_tracing() {
    let _ = fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,winit=warn")),
        )
        .with_target(false)
        .try_init();
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
        let mut world = ecs::create_world([initial_size.width, initial_size.height]);
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
        let extracted = self.world.resource::<crate::scene::ExtractedScene>().clone();
        self.renderer.render(&extracted)
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
