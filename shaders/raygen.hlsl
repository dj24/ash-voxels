#include "shared.hlsl"

[shader("raygeneration")]
void raygen_main()
{
    uint2 launch_index = DispatchRaysIndex().xy;
    uint2 launch_size = DispatchRaysDimensions().xy;
    float2 pixel_center = (float2(launch_index) + 0.5f) / float2(launch_size);
    float2 ndc = pixel_center * 2.0f - 1.0f;
    ndc.y = -ndc.y;

    float aspect = scene_uniform.viewport.z;
    float tan_half_fov = scene_uniform.viewport.w;

    float3 ray_direction = normalize(
        scene_uniform.camera_forward.xyz
        + ndc.x * aspect * tan_half_fov * scene_uniform.camera_right.xyz
        + ndc.y * tan_half_fov * scene_uniform.camera_up.xyz);

    RayDesc ray;
    ray.Origin = scene_uniform.camera_position.xyz;
    ray.Direction = ray_direction;
    ray.TMin = 0.001f;
    ray.TMax = 1000.0f;

    RayPayload payload;
    payload.color = float4(0.0f, 0.0f, 0.0f, 1.0f);

    TraceRay(
        scene_acceleration,
        RAY_FLAG_NONE,
        0xFF,
        0,
        0,
        0,
        ray,
        payload);

    output_image[launch_index] = payload.color;
}
