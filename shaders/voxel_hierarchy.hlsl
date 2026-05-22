static const uint OCCUPANCY_WORD_BITS = 32u;
static const uint REGION_AXIS = 8u;
static const uint REGION_COUNT = 512u;
static const uint MASK_WORD_COUNT = 16u;
static const uint LEAF_MASK_WORD_OFFSET = MASK_WORD_COUNT;
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

uint leaf_mask_word_offset(uint region_index)
{
    return LEAF_MASK_WORD_OFFSET + region_index * MASK_WORD_COUNT;
}
