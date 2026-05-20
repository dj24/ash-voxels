struct SceneUniform
{
    float4 camera_position;
    float4 camera_forward;
    float4 camera_right;
    float4 camera_up;
    float4 viewport;
};

struct RenderObjectData
{
    float4 bounds_min;
    float4 bounds_max;
    float4 sphere_center_radius;
    float4 voxel_size_and_padding;
};

struct RayPayload
{
    float4 color;
};

struct HitAttributes
{
    float3 normal;
};

[[vk::binding(0, 0)]] RWTexture2D<float4> output_image;
[[vk::binding(1, 0)]] RaytracingAccelerationStructure scene_acceleration;
[[vk::binding(2, 0)]] ConstantBuffer<SceneUniform> scene_uniform;
[[vk::binding(3, 0)]] StructuredBuffer<RenderObjectData> objects;

static const float PI = 3.1415926535f;
