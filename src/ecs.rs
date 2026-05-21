use std::time::Instant;

use bevy_ecs::{
    prelude::*,
    schedule::{IntoScheduleConfigs, Schedule, SystemSet},
};
use glam::Vec3;
use winit::keyboard::KeyCode;

use crate::assets::VoxelModel;
use crate::scene::{Camera, ExtractedScene, RenderObjectData, SceneUniform, VoxelProceduralObject};
use crate::terrain::terrain_grid_positions;

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AppSet {
    Input,
    Update,
    Extract,
}

#[derive(Resource, Debug)]
pub struct FrameTiming {
    pub last_frame: Instant,
    pub delta_seconds: f32,
    pub elapsed_seconds: f32,
    pub smoothed_fps: f32,
}

impl Default for FrameTiming {
    fn default() -> Self {
        Self {
            last_frame: Instant::now(),
            delta_seconds: 0.0,
            elapsed_seconds: 0.0,
            smoothed_fps: 0.0,
        }
    }
}

#[derive(Resource, Default, Debug)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Resource, Default, Debug)]
pub struct InputState {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub turn_left: bool,
    pub turn_right: bool,
    pub look_up: bool,
    pub look_down: bool,
}

impl InputState {
    pub fn set_key(&mut self, key_code: KeyCode, pressed: bool) {
        match key_code {
            KeyCode::KeyW => self.forward = pressed,
            KeyCode::KeyS => self.backward = pressed,
            KeyCode::KeyA => self.left = pressed,
            KeyCode::KeyD => self.right = pressed,
            KeyCode::Space => self.up = pressed,
            KeyCode::ShiftLeft => self.down = pressed,
            KeyCode::ArrowLeft => self.turn_left = pressed,
            KeyCode::ArrowRight => self.turn_right = pressed,
            KeyCode::ArrowUp => self.look_up = pressed,
            KeyCode::ArrowDown => self.look_down = pressed,
            _ => {}
        }
    }
}

pub fn create_world(initial_size: [u32; 2], model: &VoxelModel) -> World {
    let mut world = World::new();
    world.insert_resource(FrameTiming::default());
    world.insert_resource(WindowSize {
        width: initial_size[0],
        height: initial_size[1],
    });
    world.insert_resource(InputState::default());
    world.insert_resource(ExtractedScene::default());

    world.spawn(Camera::default());
    let object_template = VoxelProceduralObject::from(model);
    for _ in terrain_grid_positions(object_template.extent()) {
        world.spawn(object_template);
    }

    world
}

pub fn create_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.configure_sets((AppSet::Input, AppSet::Update, AppSet::Extract).chain());
    schedule.add_systems((
        update_camera.in_set(AppSet::Update),
        extract_scene.in_set(AppSet::Extract),
    ));
    schedule
}

pub fn begin_frame(world: &mut World, now: Instant, width: u32, height: u32) {
    let mut timing = world.resource_mut::<FrameTiming>();
    timing.delta_seconds = (now - timing.last_frame).as_secs_f32();
    timing.elapsed_seconds += timing.delta_seconds;
    timing.last_frame = now;
    if timing.delta_seconds > 0.0 {
        let instantaneous_fps = timing.delta_seconds.recip();
        timing.smoothed_fps = if timing.smoothed_fps == 0.0 {
            instantaneous_fps
        } else {
            timing.smoothed_fps + (instantaneous_fps - timing.smoothed_fps) * 0.1
        };
    }

    let mut window_size = world.resource_mut::<WindowSize>();
    window_size.width = width;
    window_size.height = height;
}

fn update_camera(input: Res<InputState>, timing: Res<FrameTiming>, mut query: Query<&mut Camera>) {
    let mut camera = query.single_mut().expect("single camera");
    let basis = camera.basis();
    let movement_speed = 18.0;
    let look_speed = 1.2;

    let mut movement = Vec3::ZERO;
    if input.forward {
        movement += basis.forward;
    }
    if input.backward {
        movement -= basis.forward;
    }
    if input.left {
        movement -= basis.right;
    }
    if input.right {
        movement += basis.right;
    }
    if input.up {
        movement += basis.up;
    }
    if input.down {
        movement -= basis.up;
    }

    if movement.length_squared() > 0.0 {
        camera.position += movement.normalize() * movement_speed * timing.delta_seconds;
    }

    let yaw_delta = (input.turn_left as i32 - input.turn_right as i32) as f32;
    let pitch_delta = (input.look_down as i32 - input.look_up as i32) as f32;
    camera.yaw += -yaw_delta * look_speed * timing.delta_seconds;
    camera.pitch =
        (camera.pitch + pitch_delta * look_speed * timing.delta_seconds).clamp(-1.3, 1.3);
}

fn extract_scene(
    camera_query: Query<&Camera>,
    object_query: Query<&VoxelProceduralObject>,
    timing: Res<FrameTiming>,
    window_size: Res<WindowSize>,
    mut extracted: ResMut<ExtractedScene>,
) {
    let camera = camera_query.single().expect("single camera");
    extracted.objects.clear();
    extracted
        .objects
        .extend(object_query.iter().copied().map(RenderObjectData::from));
    extracted.camera = SceneUniform::new(
        camera,
        [window_size.width.max(1), window_size.height.max(1)],
        timing.smoothed_fps,
    );
}
