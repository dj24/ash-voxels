use glam::{UVec3, Vec3};

use crate::{
    assets::{CHUNK_DIMENSIONS, VoxelModel},
    render::Renderer,
    vk::AppError,
};

pub const TERRAIN_CHUNK_DIMENSIONS: UVec3 = CHUNK_DIMENSIONS;
pub const TERRAIN_CHUNK_VOXEL_SIZE: f32 = 0.25;
pub const TERRAIN_GRID_SIDE: usize = 48;
pub const TERRAIN_GRID_HEIGHT_LAYERS: usize = 4;
pub const TERRAIN_GRID_COUNT: usize =
    TERRAIN_GRID_SIDE * TERRAIN_GRID_SIDE * TERRAIN_GRID_HEIGHT_LAYERS;

pub fn procedural_chunk_model() -> VoxelModel {
    let dimensions = TERRAIN_CHUNK_DIMENSIONS;
    let horizontal_extent =
        Vec3::new(dimensions.x as f32, 0.0, dimensions.z as f32) * TERRAIN_CHUNK_VOXEL_SIZE;
    let vertical_extent = dimensions.y as f32 * TERRAIN_CHUNK_VOXEL_SIZE;

    VoxelModel::empty_chunk(
        Vec3::new(-horizontal_extent.x * 0.5, 0.0, -horizontal_extent.z * 0.5),
        Vec3::new(
            horizontal_extent.x * 0.5,
            vertical_extent,
            horizontal_extent.z * 0.5,
        ),
        TERRAIN_CHUNK_VOXEL_SIZE,
    )
}

pub fn terrain_grid_positions(tile_extent: Vec3) -> Vec<Vec3> {
    let x_spacing = tile_extent.x.max(0.001);
    let y_spacing = tile_extent.y.max(0.001);
    let z_spacing = tile_extent.z.max(0.001);
    let half_extent_x = (TERRAIN_GRID_SIDE as f32 - 1.0) * x_spacing * 0.5;
    let half_extent_z = (TERRAIN_GRID_SIDE as f32 - 1.0) * z_spacing * 0.5;
    let mut positions = Vec::with_capacity(TERRAIN_GRID_COUNT);
    for y in 0..TERRAIN_GRID_HEIGHT_LAYERS {
        for z in 0..TERRAIN_GRID_SIDE {
            for x in 0..TERRAIN_GRID_SIDE {
                positions.push(Vec3::new(
                    x as f32 * x_spacing - half_extent_x,
                    y as f32 * y_spacing,
                    z as f32 * z_spacing - half_extent_z,
                ));
            }
        }
    }
    positions
}

pub(crate) fn populate_voxel_buffer(
    renderer: &mut Renderer,
    model: &VoxelModel,
) -> Result<(), AppError> {
    renderer.run_compute_shader(
        "terrain_gen.spv",
        c"terrain_gen_main",
        terrain_dispatch_group_counts(model.dimensions),
    )
}

fn terrain_dispatch_group_counts(dimensions: UVec3) -> [u32; 3] {
    let group_count_x = dimensions.x.div_ceil(8) * TERRAIN_GRID_SIDE as u32;
    let group_count_y = dimensions.y.div_ceil(4) * TERRAIN_GRID_HEIGHT_LAYERS as u32;
    let group_count_z = dimensions.z.div_ceil(8) * TERRAIN_GRID_SIDE as u32;
    [group_count_x, group_count_y, group_count_z]
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{
        TERRAIN_CHUNK_DIMENSIONS, TERRAIN_CHUNK_VOXEL_SIZE, TERRAIN_GRID_COUNT,
        TERRAIN_GRID_HEIGHT_LAYERS, procedural_chunk_model, terrain_grid_positions,
    };
    use crate::assets::CHUNK_OCCUPANCY_WORD_COUNT;

    #[test]
    fn procedural_chunk_matches_expected_layout() {
        let model = procedural_chunk_model();

        assert_eq!(model.dimensions, TERRAIN_CHUNK_DIMENSIONS);
        assert_eq!(model.occupancy.len(), CHUNK_OCCUPANCY_WORD_COUNT);
        assert_eq!(model.voxel_size, TERRAIN_CHUNK_VOXEL_SIZE);
        assert_eq!(model.bounds_min.y, 0.0);
        assert_eq!(
            model.bounds_max.y,
            TERRAIN_CHUNK_DIMENSIONS.y as f32 * TERRAIN_CHUNK_VOXEL_SIZE
        );
    }

    #[test]
    fn terrain_grid_is_centered_on_origin() {
        let positions = terrain_grid_positions(Vec3::new(4.0, 1.0, 6.0));

        assert_eq!(positions.len(), TERRAIN_GRID_COUNT);
        assert_eq!(TERRAIN_GRID_HEIGHT_LAYERS, 4);
        assert_eq!(
            positions.first().copied(),
            Some(Vec3::new(-94.0, 0.0, -141.0))
        );
        assert_eq!(positions.last().copied(), Some(Vec3::new(94.0, 3.0, 141.0)));
    }
}
