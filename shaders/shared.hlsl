struct SceneUniform
{
    float4 camera_position;
    float4 camera_forward;
    float4 camera_right;
    float4 camera_up;
    float4 viewport;
    float4 hud;
};

struct RenderObjectData
{
    float4 bounds_min;
    float4 bounds_max;
    float4 voxel_size_and_dimensions;
};

struct RayData
{
    float4 color;
    float3 normal;
    float ray_t;
    uint step_count;
    uint hit;
};

struct HitAttributes
{
    float3 normal;
};

[[vk::binding(0, 0)]] RWTexture2D<float4> output_image;
[[vk::binding(1, 0)]] RaytracingAccelerationStructure scene_acceleration;
[[vk::binding(2, 0)]] ConstantBuffer<SceneUniform> scene_uniform;
[[vk::binding(3, 0)]] StructuredBuffer<RenderObjectData> objects;
[[vk::binding(4, 0)]] StructuredBuffer<uint> voxel_occupancy;
[[vk::binding(5, 0)]] Texture2D<float> coarse_depth_texture;
[[vk::binding(6, 0)]] SamplerState coarse_depth_sampler;
[[vk::binding(7, 0)]] RWTexture2D<float> coarse_depth_output;

static const float PI = 3.1415926535f;
static const float COARSE_DEPTH_NEAR = 0.1f;
static const float COARSE_DEPTH_FAR = 1000.0f;

#ifndef TERRAIN_GRID_SIDE_VALUE
#define TERRAIN_GRID_SIDE_VALUE 12
#endif

#ifndef TERRAIN_GRID_HEIGHT_LAYERS_VALUE
#define TERRAIN_GRID_HEIGHT_LAYERS_VALUE 1
#endif

static const uint TERRAIN_GRID_SIDE = TERRAIN_GRID_SIDE_VALUE;
static const uint TERRAIN_GRID_HEIGHT_LAYERS = TERRAIN_GRID_HEIGHT_LAYERS_VALUE;
