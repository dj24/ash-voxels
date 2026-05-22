#include "fastnoise.hlsl"
#include "voxel_hierarchy.hlsl"

struct RenderObjectData
{
    float4 bounds_min;
    float4 bounds_max;
    float4 voxel_size_and_dimensions;
};

[[vk::binding(3, 0)]] StructuredBuffer<RenderObjectData> objects;
[[vk::binding(4, 0)]] RWStructuredBuffer<uint> voxel_occupancy;

static const uint TERRAIN_GRID_SIDE = 12u;
static const uint THREADS_X = 8u;
static const uint THREADS_Y = 4u;
static const uint THREADS_Z = 8u;

uint3 terrain_dimensions()
{
    return uint3(
        max(1, (int)objects[0].voxel_size_and_dimensions.y),
        max(1, (int)objects[0].voxel_size_and_dimensions.z),
        max(1, (int)objects[0].voxel_size_and_dimensions.w));
}

int2 terrain_tile_coordinates(uint chunk_index)
{
    return int2(chunk_index % TERRAIN_GRID_SIDE, chunk_index / TERRAIN_GRID_SIDE);
}

static const float FREQUENCY_MULTIPLIER = 0.5;

float terrain_surface_height(int2 position, uint3 dimensions, uint chunk_index)
{
    int2 tile = terrain_tile_coordinates(chunk_index);
    float2 world_position = float2(
        tile.x * (int)dimensions.x + position.x,
        tile.y * (int)dimensions.z + position.y);
    world_position -= float2(
        (float)(TERRAIN_GRID_SIDE * dimensions.x),
        (float)(TERRAIN_GRID_SIDE * dimensions.z)) * 0.5f;

    float sample_x = world_position.x;
    float sample_y = world_position.y;

    fnl_state warp = fnlCreateState(1337);
    warp.frequency = 0.005f * FREQUENCY_MULTIPLIER;
    warp.fractal_type = FNL_FRACTAL_DOMAIN_WARP_PROGRESSIVE;
    warp.octaves = 3;
    warp.lacunarity = 2.0f;
    warp.gain = 0.5f;
    warp.domain_warp_amp = 22.0f;
    fnlDomainWarp2D(warp, sample_x, sample_y);

    fnl_state broad_shape = fnlCreateState(4242);
    broad_shape.noise_type = FNL_NOISE_OPENSIMPLEX2;
    broad_shape.frequency = 0.003f * FREQUENCY_MULTIPLIER;
    broad_shape.fractal_type = FNL_FRACTAL_FBM;
    broad_shape.octaves = 5;
    broad_shape.lacunarity = 2.0f;
    broad_shape.gain = 0.52f;

    fnl_state detail_shape = fnlCreateState(9001);
    detail_shape.noise_type = FNL_NOISE_OPENSIMPLEX2;
    detail_shape.frequency = 0.01f * FREQUENCY_MULTIPLIER;
    detail_shape.fractal_type = FNL_FRACTAL_FBM;
    detail_shape.octaves = 3;
    detail_shape.lacunarity = 2.3f;
    detail_shape.gain = 0.45f;

    float plateau = saturate(fnlGetNoise2D(broad_shape, sample_x, sample_y) * 0.5f + 0.5f);
    float erosion = 1.0f - abs(fnlGetNoise2D(detail_shape, sample_x, sample_y));
    float max_height = max((float)dimensions.y - 2.0f, 1.0f);

    return clamp(2.0f + plateau * (max_height - 3.0f) + erosion * 2.5f, 1.0f, max_height);
}

[numthreads(8, 4, 8)]
void terrain_gen_main(uint3 dispatch_id : SV_DispatchThreadID)
{
    uint3 dimensions = terrain_dimensions();
    uint chunk_index = dispatch_id.z / dimensions.z;
    if (chunk_index >= TERRAIN_GRID_SIDE * TERRAIN_GRID_SIDE)
    {
        return;
    }

    uint local_z = dispatch_id.z % dimensions.z;
    if (dispatch_id.x >= dimensions.x || dispatch_id.y >= dimensions.y || local_z >= dimensions.z)
    {
        return;
    }

    uint3 local = uint3(dispatch_id.x, dispatch_id.y, local_z);
    uint3 region = uint3(local.x >> 3, local.y >> 3, local.z >> 3);
    uint region_index = flatten_region_index(region);
    uint3 leaf_local = uint3(local.x & 7u, local.y & 7u, local.z & 7u);
    uint leaf_index = flatten_leaf_index(leaf_local);
    uint word_offset = chunk_index * CHUNK_OCCUPANCY_WORD_COUNT;
    float surface_height = terrain_surface_height(int2(local.x, local.z), dimensions, chunk_index);

    if ((float)local.y <= surface_height)
    {
        InterlockedOr(
            voxel_occupancy[word_offset + occupancy_word_index(region_index)],
            occupancy_bit_mask(region_index));
        InterlockedOr(
            voxel_occupancy[word_offset + leaf_mask_word_offset(region_index) + occupancy_word_index(leaf_index)],
            occupancy_bit_mask(leaf_index));
    }
}
