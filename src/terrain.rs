use glam::{UVec3, Vec3};

use crate::{
    assets::{VoxelModel, occupancy_word_len},
    render::Renderer,
    vk::AppError,
};

pub const TERRAIN_CHUNK_DIMENSIONS: UVec3 = UVec3::new(64, 64, 64);
pub const TERRAIN_CHUNK_VOXEL_SIZE: f32 = 0.25;
pub const TERRAIN_GRID_SIDE: usize = 12;
pub const TERRAIN_GRID_COUNT: usize = TERRAIN_GRID_SIDE * TERRAIN_GRID_SIDE;

pub fn procedural_chunk_model() -> VoxelModel {
    let dimensions = TERRAIN_CHUNK_DIMENSIONS;
    let occupancy = vec![0u32; occupancy_word_len(dimensions)];
    let horizontal_extent =
        Vec3::new(dimensions.x as f32, 0.0, dimensions.z as f32) * TERRAIN_CHUNK_VOXEL_SIZE;
    let vertical_extent = dimensions.y as f32 * TERRAIN_CHUNK_VOXEL_SIZE;

    VoxelModel {
        dimensions,
        occupancy,
        bounds_min: Vec3::new(-horizontal_extent.x * 0.5, 0.0, -horizontal_extent.z * 0.5),
        bounds_max: Vec3::new(
            horizontal_extent.x * 0.5,
            vertical_extent,
            horizontal_extent.z * 0.5,
        ),
        voxel_size: TERRAIN_CHUNK_VOXEL_SIZE,
    }
}

pub fn terrain_grid_positions(tile_extent: Vec3) -> Vec<Vec3> {
    let x_spacing = tile_extent.x.max(0.001);
    let z_spacing = tile_extent.z.max(0.001);
    let half_extent_x = (TERRAIN_GRID_SIDE as f32 - 1.0) * x_spacing * 0.5;
    let half_extent_z = (TERRAIN_GRID_SIDE as f32 - 1.0) * z_spacing * 0.5;
    let mut positions = Vec::with_capacity(TERRAIN_GRID_COUNT);
    for z in 0..TERRAIN_GRID_SIDE {
        for x in 0..TERRAIN_GRID_SIDE {
            positions.push(Vec3::new(
                x as f32 * x_spacing - half_extent_x,
                0.0,
                z as f32 * z_spacing - half_extent_z,
            ));
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
    let group_count_x = dimensions.x.div_ceil(8);
    let group_count_y = dimensions.y.div_ceil(4);
    let total_depth = dimensions.z * TERRAIN_GRID_COUNT as u32;
    let group_count_z = total_depth.div_ceil(8);
    [group_count_x, group_count_y, group_count_z]
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{
        TERRAIN_CHUNK_DIMENSIONS, TERRAIN_CHUNK_VOXEL_SIZE, TERRAIN_GRID_COUNT,
        procedural_chunk_model, terrain_grid_positions,
    };
    use crate::assets::occupancy_word_len;

    #[test]
    fn procedural_chunk_matches_expected_layout() {
        let model = procedural_chunk_model();

        assert_eq!(model.dimensions, TERRAIN_CHUNK_DIMENSIONS);
        assert_eq!(
            model.occupancy.len(),
            occupancy_word_len(TERRAIN_CHUNK_DIMENSIONS)
        );
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
        assert_eq!(
            positions.first().copied(),
            Some(Vec3::new(-22.0, 0.0, -33.0))
        );
        assert_eq!(positions.last().copied(), Some(Vec3::new(22.0, 0.0, 33.0)));
    }
}
