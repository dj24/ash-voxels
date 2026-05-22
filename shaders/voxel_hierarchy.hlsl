static const uint OCCUPANCY_WORD_BITS = 32u;
static const uint REGION_AXIS = 8u;
static const uint REGION_COUNT = 512u;
static const uint MASK_WORD_COUNT = 16u;
static const uint MIP64_REGION_AXIS = 4u;
static const uint MIP64_REGION_COUNT = 64u;
static const uint MIP64_WORD_COUNT = 2u;
static const uint MIP8_REGION_AXIS = 2u;
static const uint MIP8_REGION_COUNT = 8u;
static const uint MIP8_WORD_COUNT = 1u;
static const uint REGION_MASK_WORD_OFFSET = 0u;
static const uint MIP64_MASK_WORD_OFFSET = REGION_MASK_WORD_OFFSET + MASK_WORD_COUNT;
static const uint MIP8_MASK_WORD_OFFSET = MIP64_MASK_WORD_OFFSET + MIP64_WORD_COUNT;
static const uint LEAF_MASK_WORD_OFFSET = MIP8_MASK_WORD_OFFSET + MIP8_WORD_COUNT;
static const uint CHUNK_OCCUPANCY_WORD_COUNT = LEAF_MASK_WORD_OFFSET + REGION_COUNT * MASK_WORD_COUNT;

uint flatten_region_index(uint3 region_position)
{
    return region_position.x + REGION_AXIS * (region_position.y + REGION_AXIS * region_position.z);
}

uint flatten_leaf_index(uint3 local_position)
{
    return local_position.x + REGION_AXIS * (local_position.y + REGION_AXIS * local_position.z);
}

uint occupancy_word_index(uint bit_index)
{
    return bit_index / OCCUPANCY_WORD_BITS;
}

uint occupancy_bit_mask(uint bit_index)
{
    return 1u << (bit_index % OCCUPANCY_WORD_BITS);
}

uint flatten_mip64_region_index(uint3 region_position)
{
    return region_position.x
        + MIP64_REGION_AXIS * (region_position.y + MIP64_REGION_AXIS * region_position.z);
}

uint flatten_mip8_region_index(uint3 region_position)
{
    return region_position.x
        + MIP8_REGION_AXIS * (region_position.y + MIP8_REGION_AXIS * region_position.z);
}

bool mip64_mask_bit_is_set(uint occupancy_word_offset, uint region_index)
{
    uint word_index = occupancy_word_index(region_index);
    uint bit_mask = occupancy_bit_mask(region_index);
    return (voxel_occupancy[occupancy_word_offset + MIP64_MASK_WORD_OFFSET + word_index] & bit_mask) != 0u;
}

bool mip8_mask_bit_is_set(uint occupancy_word_offset, uint region_index)
{
    uint word_index = occupancy_word_index(region_index);
    uint bit_mask = occupancy_bit_mask(region_index);
    return (voxel_occupancy[occupancy_word_offset + MIP8_MASK_WORD_OFFSET + word_index] & bit_mask) != 0u;
}

uint leaf_mask_word_offset(uint region_index)
{
    return LEAF_MASK_WORD_OFFSET + region_index * MASK_WORD_COUNT;
}
