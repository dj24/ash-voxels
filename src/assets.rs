use glam::{UVec3, Vec3};

pub const CHUNK_DIMENSIONS: UVec3 = UVec3::new(64, 64, 64);
pub const REGION_DIMENSIONS: UVec3 = UVec3::new(8, 8, 8);
pub const REGION_GRID_DIMENSIONS: UVec3 = UVec3::new(8, 8, 8);
pub const REGION_COUNT: usize = 512;
pub const MASK_WORD_BITS: usize = u32::BITS as usize;
pub const MASK_WORD_COUNT: usize = 16;
pub const LEAF_MASK_WORD_OFFSET: usize = MASK_WORD_COUNT;
pub const CHUNK_OCCUPANCY_WORD_COUNT: usize = MASK_WORD_COUNT + REGION_COUNT * MASK_WORD_COUNT;

#[derive(Clone, Debug)]
pub struct VoxelModel {
    pub dimensions: UVec3,
    pub occupancy: Vec<u32>,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub voxel_size: f32,
}

impl VoxelModel {
    pub fn empty_chunk(bounds_min: Vec3, bounds_max: Vec3, voxel_size: f32) -> Self {
        Self {
            dimensions: CHUNK_DIMENSIONS,
            occupancy: empty_chunk_occupancy(),
            bounds_min,
            bounds_max,
            voxel_size,
        }
    }

    pub fn extent(&self) -> Vec3 {
        self.bounds_max - self.bounds_min
    }

    pub fn voxel_count(&self) -> usize {
        chunk_voxel_count()
    }

    pub fn occupancy_word_count(&self) -> usize {
        self.occupancy.len()
    }

    pub fn occupancy_size_bytes(&self) -> usize {
        self.occupancy.len() * std::mem::size_of::<u32>()
    }

    pub fn occupancy_size_kib(&self) -> f32 {
        self.occupancy_size_bytes() as f32 / 1024.0
    }
}

pub fn chunk_voxel_count() -> usize {
    CHUNK_DIMENSIONS.x as usize * CHUNK_DIMENSIONS.y as usize * CHUNK_DIMENSIONS.z as usize
}

pub fn empty_chunk_occupancy() -> Vec<u32> {
    vec![0u32; CHUNK_OCCUPANCY_WORD_COUNT]
}

pub fn flatten_region_index(region_position: UVec3) -> usize {
    debug_assert!(region_position.x < REGION_GRID_DIMENSIONS.x);
    debug_assert!(region_position.y < REGION_GRID_DIMENSIONS.y);
    debug_assert!(region_position.z < REGION_GRID_DIMENSIONS.z);

    region_position.x as usize
        + REGION_GRID_DIMENSIONS.x as usize
            * (region_position.y as usize
                + REGION_GRID_DIMENSIONS.y as usize * region_position.z as usize)
}

pub fn flatten_leaf_index(local_position: UVec3) -> usize {
    debug_assert!(local_position.x < REGION_DIMENSIONS.x);
    debug_assert!(local_position.y < REGION_DIMENSIONS.y);
    debug_assert!(local_position.z < REGION_DIMENSIONS.z);

    local_position.x as usize
        + REGION_DIMENSIONS.x as usize
            * (local_position.y as usize + REGION_DIMENSIONS.y as usize * local_position.z as usize)
}

pub fn occupancy_word_and_mask(index: usize) -> (usize, u32) {
    let word_index = index / MASK_WORD_BITS;
    let bit_index = index % MASK_WORD_BITS;
    (word_index, 1u32 << bit_index)
}

pub fn region_leaf_word_offset(region_index: usize) -> usize {
    debug_assert!(region_index < REGION_COUNT);
    LEAF_MASK_WORD_OFFSET + region_index * MASK_WORD_COUNT
}

pub fn region_mask_bit_is_set(occupancy: &[u32], region_index: usize) -> bool {
    let (word_index, bit_mask) = occupancy_word_and_mask(region_index);
    occupancy[word_index] & bit_mask != 0
}

pub fn occupancy_bit_is_set(occupancy: &[u32], position: UVec3) -> bool {
    debug_assert!(position.x < CHUNK_DIMENSIONS.x);
    debug_assert!(position.y < CHUNK_DIMENSIONS.y);
    debug_assert!(position.z < CHUNK_DIMENSIONS.z);

    let region_position = UVec3::new(position.x / 8, position.y / 8, position.z / 8);
    let region_index = flatten_region_index(region_position);
    if !region_mask_bit_is_set(occupancy, region_index) {
        return false;
    }

    let leaf_local = UVec3::new(position.x & 7, position.y & 7, position.z & 7);
    let leaf_index = flatten_leaf_index(leaf_local);
    let (word_index, bit_mask) = occupancy_word_and_mask(leaf_index);
    occupancy[region_leaf_word_offset(region_index) + word_index] & bit_mask != 0
}

pub fn set_occupancy_bit(occupancy: &mut [u32], position: UVec3) {
    debug_assert!(position.x < CHUNK_DIMENSIONS.x);
    debug_assert!(position.y < CHUNK_DIMENSIONS.y);
    debug_assert!(position.z < CHUNK_DIMENSIONS.z);

    let region_position = UVec3::new(position.x / 8, position.y / 8, position.z / 8);
    let region_index = flatten_region_index(region_position);
    let (region_word_index, region_bit_mask) = occupancy_word_and_mask(region_index);
    occupancy[region_word_index] |= region_bit_mask;

    let leaf_local = UVec3::new(position.x & 7, position.y & 7, position.z & 7);
    let leaf_index = flatten_leaf_index(leaf_local);
    let (leaf_word_index, leaf_bit_mask) = occupancy_word_and_mask(leaf_index);
    occupancy[region_leaf_word_offset(region_index) + leaf_word_index] |= leaf_bit_mask;
}

#[cfg(test)]
mod tests {
    use glam::{UVec3, Vec3};

    use super::{
        CHUNK_DIMENSIONS, CHUNK_OCCUPANCY_WORD_COUNT, MASK_WORD_COUNT, REGION_COUNT, VoxelModel,
        empty_chunk_occupancy, flatten_leaf_index, flatten_region_index, occupancy_bit_is_set,
        region_leaf_word_offset, region_mask_bit_is_set, set_occupancy_bit,
    };

    #[test]
    fn empty_chunk_storage_matches_fixed_layout() {
        let occupancy = empty_chunk_occupancy();

        assert_eq!(occupancy.len(), CHUNK_OCCUPANCY_WORD_COUNT);
        assert!(occupancy.iter().all(|word| *word == 0));
    }

    #[test]
    fn setting_one_voxel_marks_one_region_and_one_leaf_bit() {
        let mut occupancy = empty_chunk_occupancy();
        let voxel = UVec3::new(9, 2, 17);

        set_occupancy_bit(&mut occupancy, voxel);

        let region_index = flatten_region_index(UVec3::new(1, 0, 2));
        assert!(region_mask_bit_is_set(&occupancy, region_index));
        assert_eq!(
            occupancy[..MASK_WORD_COUNT]
                .iter()
                .filter(|word| **word != 0)
                .count(),
            1
        );
        assert!(occupancy_bit_is_set(&occupancy, voxel));
        assert_eq!(
            occupancy[region_leaf_word_offset(region_index)
                ..region_leaf_word_offset(region_index) + MASK_WORD_COUNT]
                .iter()
                .filter(|word| **word != 0)
                .count(),
            1
        );
    }

    #[test]
    fn voxels_in_same_region_share_region_mask_but_use_distinct_leaf_bits() {
        let mut occupancy = empty_chunk_occupancy();
        let first = UVec3::new(8, 8, 8);
        let second = UVec3::new(15, 15, 15);

        set_occupancy_bit(&mut occupancy, first);
        set_occupancy_bit(&mut occupancy, second);

        let region_index = flatten_region_index(UVec3::new(1, 1, 1));
        assert!(region_mask_bit_is_set(&occupancy, region_index));
        assert!(occupancy_bit_is_set(&occupancy, first));
        assert!(occupancy_bit_is_set(&occupancy, second));
        assert_eq!(
            occupancy[..MASK_WORD_COUNT]
                .iter()
                .filter(|word| **word != 0)
                .count(),
            1
        );
        assert_ne!(
            flatten_leaf_index(UVec3::new(first.x & 7, first.y & 7, first.z & 7)),
            flatten_leaf_index(UVec3::new(second.x & 7, second.y & 7, second.z & 7))
        );
    }

    #[test]
    fn voxels_in_different_regions_use_distinct_region_and_leaf_offsets() {
        let mut occupancy = empty_chunk_occupancy();
        let first = UVec3::new(0, 0, 0);
        let second = UVec3::new(63, 63, 63);

        set_occupancy_bit(&mut occupancy, first);
        set_occupancy_bit(&mut occupancy, second);

        let first_region = flatten_region_index(UVec3::new(0, 0, 0));
        let second_region = flatten_region_index(UVec3::new(7, 7, 7));
        assert_ne!(first_region, second_region);
        assert_ne!(
            region_leaf_word_offset(first_region),
            region_leaf_word_offset(second_region)
        );
        assert!(occupancy_bit_is_set(&occupancy, first));
        assert!(occupancy_bit_is_set(&occupancy, second));
        assert_eq!(
            occupancy[..MASK_WORD_COUNT]
                .iter()
                .filter(|word| **word != 0)
                .count(),
            2
        );
    }

    #[test]
    fn empty_chunk_model_uses_fixed_chunk_dimensions() {
        let model = VoxelModel::empty_chunk(Vec3::ZERO, Vec3::ONE, 0.25);

        assert_eq!(model.dimensions, CHUNK_DIMENSIONS);
        assert_eq!(model.occupancy_word_count(), CHUNK_OCCUPANCY_WORD_COUNT);
        assert_eq!(model.voxel_count(), 64 * 64 * 64);
        assert_eq!(REGION_COUNT, 512);
    }
}
