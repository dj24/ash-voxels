#include "shared.hlsl"
#include "voxel_intersection.hlsl"

float3 camera_ray_direction(float2 uv)
{
    float2 ndc = uv * 2.0f - 1.0f;
    ndc.y = -ndc.y;

    float aspect = scene_uniform.viewport.z;
    float tan_half_fov = scene_uniform.viewport.w;

    return normalize(
        scene_uniform.camera_forward.xyz
        + ndc.x * aspect * tan_half_fov * scene_uniform.camera_right.xyz
        + ndc.y * tan_half_fov * scene_uniform.camera_up.xyz);
}

[numthreads(8, 8, 1)]
void coarse_depth_trace_main(uint3 dispatch_id : SV_DispatchThreadID)
{
    uint coarse_width, coarse_height;
    coarse_depth_output.GetDimensions(coarse_width, coarse_height);

    if (dispatch_id.x >= coarse_width || dispatch_id.y >= coarse_height)
    {
        return;
    }

    float2 uv = (float2(dispatch_id.xy) + 0.5f) / float2(coarse_width, coarse_height);

    RayDesc ray;
    ray.Origin = scene_uniform.camera_position.xyz;
    ray.Direction = camera_ray_direction(uv);
    ray.TMin = 0.001f;
    ray.TMax = 1000.0f;

    float coarse_depth = 0.0f;
    RayQuery<RAY_FLAG_NONE> ray_query;
    ray_query.TraceRayInline(scene_acceleration, RAY_FLAG_NONE, 0xFF, ray);

    [loop]
    while (ray_query.Proceed())
    {
        if (ray_query.CandidateType() != CANDIDATE_PROCEDURAL_PRIMITIVE)
        {
            continue;
        }

        uint instance_id = ray_query.CandidateInstanceID();
        RenderObjectData object = objects[instance_id];
        float candidate_hit_t;
        float3 candidate_normal;
        uint candidate_step_count;

        if (!intersect_voxel_object(
            ray_query.CandidateObjectRayOrigin(),
            ray_query.CandidateObjectRayDirection(),
            ray.TMin,
            ray_query.CommittedRayT(),
            object,
            instance_id,
            candidate_hit_t,
            candidate_normal,
            candidate_step_count))
        {
            continue;
        }

        ray_query.CommitProceduralPrimitiveHit(candidate_hit_t);
        coarse_depth = candidate_hit_t;
    }

    coarse_depth_output[dispatch_id.xy] = coarse_depth;
}
