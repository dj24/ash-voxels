#include "shared.hlsl"

static const float3 BOX_CORNERS[8] = {
    float3(0.0f, 0.0f, 0.0f),
    float3(1.0f, 0.0f, 0.0f),
    float3(1.0f, 1.0f, 0.0f),
    float3(0.0f, 1.0f, 0.0f),
    float3(0.0f, 0.0f, 1.0f),
    float3(1.0f, 0.0f, 1.0f),
    float3(1.0f, 1.0f, 1.0f),
    float3(0.0f, 1.0f, 1.0f),
};

static const uint BOX_INDICES[36] = {
    0, 2, 1, 0, 3, 2,
    4, 5, 6, 4, 6, 7,
    0, 1, 5, 0, 5, 4,
    2, 3, 7, 2, 7, 6,
    1, 2, 6, 1, 6, 5,
    3, 0, 4, 3, 4, 7,
};

float3 terrain_grid_translation(uint instance_id, float3 extent)
{
    uint grid_x = instance_id % TERRAIN_GRID_SIDE;
    uint grid_z = instance_id / TERRAIN_GRID_SIDE;
    float half_extent_x = (TERRAIN_GRID_SIDE - 1u) * extent.x * 0.5f;
    float half_extent_z = (TERRAIN_GRID_SIDE - 1u) * extent.z * 0.5f;

    return float3(
        (float)grid_x * extent.x - half_extent_x,
        0.0f,
        (float)grid_z * extent.z - half_extent_z);
}

struct VsOutput
{
    float4 position : SV_Position;
};

VsOutput coarse_depth_prepass_main(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID)
{
    RenderObjectData object = objects[instance_id];
    float3 bounds_min = object.bounds_min.xyz;
    float3 bounds_max = object.bounds_max.xyz;
    float3 extent = bounds_max - bounds_min;
    float3 local_corner = lerp(bounds_min, bounds_max, BOX_CORNERS[BOX_INDICES[vertex_id]]);
    float3 world_position = local_corner + terrain_grid_translation(instance_id, extent);

    float3 relative = world_position - scene_uniform.camera_position.xyz;
    float3 view_position = float3(
        dot(relative, scene_uniform.camera_right.xyz),
        dot(relative, scene_uniform.camera_up.xyz),
        -dot(relative, scene_uniform.camera_forward.xyz));

    float clip_x = view_position.x / (scene_uniform.viewport.z * scene_uniform.viewport.w);
    float clip_y = -view_position.y / scene_uniform.viewport.w;
    float clip_z =
        (COARSE_DEPTH_FAR / (COARSE_DEPTH_NEAR - COARSE_DEPTH_FAR)) * view_position.z
        + ((COARSE_DEPTH_FAR * COARSE_DEPTH_NEAR) / (COARSE_DEPTH_NEAR - COARSE_DEPTH_FAR));

    VsOutput output;
    output.position = float4(clip_x, clip_y, clip_z, -view_position.z);
    return output;
}
