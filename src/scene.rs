use std::f32::consts::FRAC_PI_3;

use bevy_ecs::prelude::{Component, Resource};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, UVec3, Vec3, Vec4};

use crate::assets::VoxelModel;

#[derive(Component, Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub vertical_fov_radians: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, 18.0, 72.0),
            yaw: 0.0,
            pitch: -0.28,
            vertical_fov_radians: FRAC_PI_3,
        }
    }
}

impl Camera {
    pub fn basis(&self) -> CameraBasis {
        let rotation = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch);
        let forward = rotation * -Vec3::Z;
        let right = rotation * Vec3::X;
        let up = rotation * Vec3::Y;
        CameraBasis {
            forward: forward.normalize_or_zero(),
            right: right.normalize_or_zero(),
            up: up.normalize_or_zero(),
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        let basis = self.basis();
        Mat4::look_to_rh(self.position, basis.forward, basis.up)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CameraBasis {
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct VoxelProceduralObject {
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub voxel_size: f32,
    pub voxel_dimensions: UVec3,
}

pub const SPHERE_GRID_SIDE: usize = 100;
pub const SPHERE_GRID_COUNT: usize = SPHERE_GRID_SIDE * SPHERE_GRID_SIDE;
pub const SPHERE_GRID_SPACING: f32 = 1.1;

impl From<&VoxelModel> for VoxelProceduralObject {
    fn from(value: &VoxelModel) -> Self {
        Self {
            bounds_min: value.bounds_min,
            bounds_max: value.bounds_max,
            voxel_size: value.voxel_size,
            voxel_dimensions: value.dimensions,
        }
    }
}

impl VoxelProceduralObject {
    pub fn extent(&self) -> Vec3 {
        self.bounds_max - self.bounds_min
    }

    pub fn voxel_dimensions(&self) -> UVec3 {
        self.voxel_dimensions
    }
}

pub fn sphere_grid_positions() -> Vec<Vec3> {
    let half_extent = (SPHERE_GRID_SIDE as f32 - 1.0) * SPHERE_GRID_SPACING * 0.5;
    let mut positions = Vec::with_capacity(SPHERE_GRID_COUNT);
    for z in 0..SPHERE_GRID_SIDE {
        for x in 0..SPHERE_GRID_SIDE {
            positions.push(Vec3::new(
                x as f32 * SPHERE_GRID_SPACING - half_extent,
                0.0,
                z as f32 * SPHERE_GRID_SPACING - half_extent,
            ));
        }
    }
    positions
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SceneUniform {
    pub camera_position: Vec4,
    pub camera_forward: Vec4,
    pub camera_right: Vec4,
    pub camera_up: Vec4,
    pub viewport: Vec4,
    pub hud: Vec4,
}

impl SceneUniform {
    pub fn new(camera: &Camera, viewport_extent: [u32; 2], fps: f32) -> Self {
        let basis = camera.basis();
        let width = viewport_extent[0].max(1) as f32;
        let height = viewport_extent[1].max(1) as f32;
        let aspect = width / height;
        let tan_half_fov = (camera.vertical_fov_radians * 0.5).tan();

        Self {
            camera_position: camera.position.extend(1.0),
            camera_forward: basis.forward.extend(0.0),
            camera_right: basis.right.extend(0.0),
            camera_up: basis.up.extend(0.0),
            viewport: Vec4::new(width, height, aspect, tan_half_fov),
            hud: Vec4::new(fps.max(0.0), 0.0, 0.0, 0.0),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct RenderObjectData {
    pub bounds_min: Vec4,
    pub bounds_max: Vec4,
    pub voxel_size_and_dimensions: Vec4,
}

impl From<VoxelProceduralObject> for RenderObjectData {
    fn from(value: VoxelProceduralObject) -> Self {
        let dims = value.voxel_dimensions();
        Self {
            bounds_min: value.bounds_min.extend(0.0),
            bounds_max: value.bounds_max.extend(0.0),
            voxel_size_and_dimensions: Vec4::new(
                value.voxel_size,
                dims.x as f32,
                dims.y as f32,
                dims.z as f32,
            ),
        }
    }
}

#[derive(Clone, Debug, Default, Resource)]
pub struct ExtractedScene {
    pub camera: SceneUniform,
    pub objects: Vec<RenderObjectData>,
}

#[cfg(test)]
mod tests {
    use super::VoxelProceduralObject;
    use glam::{UVec3, Vec3};

    #[test]
    fn voxel_dimensions_are_preserved() {
        let object = VoxelProceduralObject {
            bounds_min: Vec3::new(-1.0, -1.0, -1.0),
            bounds_max: Vec3::new(1.0, 1.0, 1.0),
            voxel_size: 0.3,
            voxel_dimensions: UVec3::new(7, 7, 7),
        };

        assert_eq!(object.voxel_dimensions(), UVec3::new(7, 7, 7));
    }

    #[test]
    fn extent_is_still_computed_from_bounds() {
        let object = VoxelProceduralObject {
            bounds_min: Vec3::new(-0.45, -0.2, -0.1),
            bounds_max: Vec3::new(0.45, 0.2, 0.1),
            voxel_size: 0.5,
            voxel_dimensions: UVec3::new(9, 4, 2),
        };

        assert_eq!(object.extent(), Vec3::new(0.9, 0.4, 0.2));
    }
}
