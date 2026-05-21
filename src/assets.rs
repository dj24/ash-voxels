use std::{
    fs,
    path::{Path, PathBuf},
};

use glam::{UVec3, Vec3};

use crate::vk::AppError;

pub const DRAGON_ASSET_PATH: &str = "assets/dragon.vox";
pub const MODEL_TARGET_LONGEST_AXIS: f32 = 0.9;
const OCCUPANCY_WORD_BITS: usize = u32::BITS as usize;

#[derive(Clone, Debug)]
pub struct VoxelModel {
    pub dimensions: UVec3,
    pub occupancy: Vec<u32>,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub voxel_size: f32,
}

impl VoxelModel {
    pub fn load_dragon() -> Result<Self, AppError> {
        let asset_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DRAGON_ASSET_PATH);
        Self::load_from_file(&asset_path)
    }

    pub fn load_from_file(path: &Path) -> Result<Self, AppError> {
        let bytes = fs::read(path)?;
        let data = dot_vox::load_bytes(&bytes).map_err(|error| {
            AppError::Message(format!(
                "failed to load voxel asset {}: {error}",
                path.display()
            ))
        })?;
        let model = data.models.first().ok_or_else(|| {
            AppError::Message(format!(
                "voxel asset {} did not contain any models",
                path.display()
            ))
        })?;

        Self::from_dot_vox_model(model)
    }

    fn from_dot_vox_model(model: &dot_vox::Model) -> Result<Self, AppError> {
        if model.voxels.is_empty() {
            return Err(AppError::Message(
                "voxel model did not contain any occupied voxels".to_string(),
            ));
        }

        let occupied_positions = model
            .voxels
            .iter()
            .map(|voxel| UVec3::new(voxel.x as u32, voxel.z as u32, voxel.y as u32))
            .collect::<Vec<_>>();

        let mut occupied_min = occupied_positions[0];
        let mut occupied_max = occupied_positions[0];
        for position in &occupied_positions[1..] {
            occupied_min = occupied_min.min(*position);
            occupied_max = occupied_max.max(*position);
        }

        let dimensions = occupied_max - occupied_min + UVec3::ONE;
        let longest_axis = dimensions.max_element().max(1) as f32;
        let voxel_size = MODEL_TARGET_LONGEST_AXIS / longest_axis;
        let extent = dimensions.as_vec3() * voxel_size;
        let bounds_min = -extent * 0.5;
        let bounds_max = extent * 0.5;

        let mut occupancy = vec![0u32; occupancy_word_len(dimensions)];
        for position in occupied_positions {
            let local = position - occupied_min;
            set_occupancy_bit(&mut occupancy, flatten_index(local, dimensions));
        }

        Ok(Self {
            dimensions,
            occupancy,
            bounds_min,
            bounds_max,
            voxel_size,
        })
    }

    pub fn extent(&self) -> Vec3 {
        self.bounds_max - self.bounds_min
    }

    pub fn voxel_count(&self) -> usize {
        voxel_count(self.dimensions)
    }

    pub fn occupancy_word_count(&self) -> usize {
        occupancy_word_len(self.dimensions)
    }

    pub fn occupancy_size_bytes(&self) -> usize {
        self.occupancy.len() * std::mem::size_of::<u32>()
    }

    pub fn occupancy_size_kib(&self) -> f32 {
        self.occupancy_size_bytes() as f32 / 1024.0
    }
}

pub fn voxel_count(dimensions: UVec3) -> usize {
    dimensions.x as usize * dimensions.y as usize * dimensions.z as usize
}

pub fn occupancy_word_len(dimensions: UVec3) -> usize {
    voxel_count(dimensions).div_ceil(OCCUPANCY_WORD_BITS)
}

pub fn flatten_index(position: UVec3, dimensions: UVec3) -> usize {
    position.x as usize
        + dimensions.x as usize
            * (position.y as usize + dimensions.y as usize * position.z as usize)
}

pub fn occupancy_word_and_mask(index: usize) -> (usize, u32) {
    let word_index = index / OCCUPANCY_WORD_BITS;
    let bit_index = index % OCCUPANCY_WORD_BITS;
    (word_index, 1u32 << bit_index)
}

pub fn set_occupancy_bit(occupancy: &mut [u32], index: usize) {
    let (word_index, bit_mask) = occupancy_word_and_mask(index);
    occupancy[word_index] |= bit_mask;
}

pub fn occupancy_bit_is_set(occupancy: &[u32], index: usize) -> bool {
    let (word_index, bit_mask) = occupancy_word_and_mask(index);
    occupancy[word_index] & bit_mask != 0
}

#[cfg(test)]
mod tests {
    use glam::UVec3;

    use super::{
        MODEL_TARGET_LONGEST_AXIS, VoxelModel, occupancy_bit_is_set, occupancy_word_len,
        set_occupancy_bit, voxel_count,
    };

    #[test]
    fn dragon_model_loads() {
        let model = VoxelModel::load_dragon().expect("dragon.vox should load");
        assert!(model.dimensions.max_element() > 0);
        assert!(model.occupancy.iter().any(|value| *value != 0));
    }

    #[test]
    fn dragon_occupancy_matches_dimensions() {
        let model = VoxelModel::load_dragon().expect("dragon.vox should load");
        let expected_len = occupancy_word_len(model.dimensions);

        assert_eq!(model.occupancy.len(), expected_len);
    }

    #[test]
    fn dragon_bounds_fit_target_extent() {
        let model = VoxelModel::load_dragon().expect("dragon.vox should load");
        let longest_axis = model.extent().max_element();

        assert!(longest_axis <= MODEL_TARGET_LONGEST_AXIS + f32::EPSILON);
    }

    #[test]
    fn occupancy_word_count_rounds_up_to_full_words() {
        let dimensions = UVec3::new(3, 3, 4);

        assert_eq!(voxel_count(dimensions), 36);
        assert_eq!(occupancy_word_len(dimensions), 2);
    }

    #[test]
    fn occupancy_bits_can_cross_word_boundaries() {
        let mut occupancy = vec![0u32; occupancy_word_len(UVec3::new(13, 1, 5))];
        let indices = [0usize, 31, 32, 64];

        for index in indices {
            set_occupancy_bit(&mut occupancy, index);
        }

        for index in indices {
            assert!(occupancy_bit_is_set(&occupancy, index));
        }
        assert!(!occupancy_bit_is_set(&occupancy, 30));
        assert!(!occupancy_bit_is_set(&occupancy, 33));
    }
}
