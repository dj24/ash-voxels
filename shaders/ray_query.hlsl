#include "shared.hlsl"
#include "voxel_intersection.hlsl"

uint glyph_mask(int glyph)
{
    switch (glyph)
    {
        case 0: return 31599u;
        case 1: return 29850u;
        case 2: return 29671u;
        case 3: return 31207u;
        case 4: return 18925u;
        case 5: return 31183u;
        case 6: return 31695u;
        case 7: return 18727u;
        case 8: return 31727u;
        case 9: return 31215u;
        case 10: return 4815u;
        case 11: return 4843u;
        case 12: return 31183u;
        default: return 0u;
    }
}

bool glyph_contains_pixel(int2 local, uint mask, int scale)
{
    if (local.x < 0 || local.y < 0)
    {
        return false;
    }

    int column = local.x / scale;
    int row = local.y / scale;
    if (column >= 3 || row >= 5)
    {
        return false;
    }

    int inset = max(scale - 1, 1);
    if ((local.x % scale) >= inset || (local.y % scale) >= inset)
    {
        return false;
    }

    uint bit = 1u << (row * 3 + column);
    return (mask & bit) != 0u;
}

float4 overlay_fps_counter(uint2 launch_index, uint2 launch_size, float4 color)
{
    const int scale = 4;
    const int glyph_width = scale * 3;
    const int glyph_height = scale * 5;
    const int glyph_gap = 3;
    const int padding = 6;
    const int panel_margin = 12;
    const int slot_count = 8;

    int2 panel_size = int2(
        padding * 2 + slot_count * glyph_width + (slot_count - 1) * glyph_gap,
        padding * 2 + glyph_height);
    int2 panel_min = int2((int)launch_size.x - panel_size.x - panel_margin, panel_margin);
    int2 panel_max = panel_min + panel_size;
    int2 pixel = int2(launch_index);

    if (pixel.x < panel_min.x || pixel.y < panel_min.y || pixel.x >= panel_max.x || pixel.y >= panel_max.y)
    {
        return color;
    }

    int fps = clamp((int)round(scene_uniform.hud.x), 0, 9999);
    int glyphs[slot_count] = {
        10,
        11,
        12,
        -1,
        fps >= 1000 ? fps / 1000 : -1,
        fps >= 100 ? (fps / 100) % 10 : -1,
        fps >= 10 ? (fps / 10) % 10 : -1,
        fps % 10
    };

    float overlay_alpha = 0.6f;
    float text_alpha = 0.0f;
    int2 local = pixel - panel_min - int2(padding, padding);

    [unroll]
    for (int slot = 0; slot < slot_count; ++slot)
    {
        int glyph = glyphs[slot];
        if (glyph < 0)
        {
            continue;
        }

        int glyph_x = slot * (glyph_width + glyph_gap);
        int2 glyph_local = local - int2(glyph_x, 0);
        if (glyph_contains_pixel(glyph_local, glyph_mask(glyph), scale))
        {
            text_alpha = 1.0f;
            break;
        }
    }

    float border = (
        pixel.x == panel_min.x
        || pixel.y == panel_min.y
        || pixel.x == panel_max.x - 1
        || pixel.y == panel_max.y - 1)
        ? 1.0f
        : 0.0f;
    float3 background = lerp(float3(0.05f, 0.06f, 0.08f), float3(0.25f, 0.29f, 0.34f), border);
    float3 composited = lerp(color.rgb, background, overlay_alpha);
    composited = lerp(composited, float3(0.96f, 0.98f, 1.0f), text_alpha);

    return float4(composited, color.a);
}

float4 shade_sky(float3 direction)
{
    float sky = 0.5f * (direction.y + 1.0f);
    float3 color = lerp(float3(0.12f, 0.15f, 0.2f), float3(0.65f, 0.75f, 0.95f), sky);
    return float4(color, 1.0f);
}

float4 shade_voxel(float3 normal)
{
    float3 light_direction = normalize(float3(0.45f, 0.8f, 0.35f));
    float ndotl = saturate(dot(normalize(normal), light_direction));
    float diffuse = 0.15f + ndotl * 0.85f;
    float3 albedo = float3(0.34f, 0.52f, 0.28f);
    return float4(albedo * diffuse, 1.0f);
}

float3 heatmap_ramp(float t)
{
    float3 cool = float3(0.04f, 0.05f, 0.08f);
    float3 blue = float3(0.05f, 0.32f, 0.95f);
    float3 cyan = float3(0.05f, 0.9f, 0.95f);
    float3 yellow = float3(1.0f, 0.9f, 0.15f);
    float3 hot = float3(1.0f, 0.18f, 0.08f);

    t = saturate(t);
    if (t < 0.25f)
    {
        return lerp(cool, blue, t / 0.25f);
    }
    if (t < 0.5f)
    {
        return lerp(blue, cyan, (t - 0.25f) / 0.25f);
    }
    if (t < 0.75f)
    {
        return lerp(cyan, yellow, (t - 0.5f) / 0.25f);
    }
    return lerp(yellow, hot, (t - 0.75f) / 0.25f);
}

float4 shade_ray_complexity(float4 base_color, uint step_count)
{
    const float max_visualized_steps = 512.0f;
    float normalized_steps = saturate(log2((float)step_count + 1.0f) / log2(max_visualized_steps + 1.0f));
    float3 heatmap = heatmap_ramp(normalized_steps);
    float3 debug_color = lerp(base_color.rgb * 0.18f, heatmap, 0.88f);
    return float4(debug_color, base_color.a);
}

RayData trace_voxel_scene(RayDesc ray)
{
    RayData best_hit;
    best_hit.color = shade_sky(ray.Direction);
    best_hit.normal = float3(0.0f, 1.0f, 0.0f);
    best_hit.ray_t = ray.TMax;
    best_hit.step_count = 0u;
    best_hit.hit = 0u;

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
            best_hit.step_count += candidate_step_count;
            continue;
        }

        best_hit.step_count += candidate_step_count;
        ray_query.CommitProceduralPrimitiveHit(candidate_hit_t);
        best_hit.hit = 1u;
        best_hit.ray_t = candidate_hit_t;
        best_hit.normal = candidate_normal;
        best_hit.color = shade_voxel(candidate_normal);
    }

    return best_hit;
}

float sample_min_coarse_depth(float2 uv)
{
    uint coarse_width, coarse_height;
    coarse_depth_texture.GetDimensions(coarse_width, coarse_height);
    if (coarse_width == 0 || coarse_height == 0)
    {
        return 0.0f;
    }

    int2 coarse_size = int2(coarse_width, coarse_height);
    int2 center = clamp(int2(uv * float2(coarse_width, coarse_height)), int2(0, 0), coarse_size - 1);
    float min_depth = 0.0f;

    [unroll]
    for (int dy = -1; dy <= 1; ++dy)
    {
        [unroll]
        for (int dx = -1; dx <= 1; ++dx)
        {
            int2 sample_coord = clamp(center + int2(dx, dy), int2(0, 0), coarse_size - 1);
            float sample_depth = coarse_depth_texture.Load(int3(sample_coord, 0));
            if (sample_depth <= 0.0f)
            {
                continue;
            }

            min_depth = min_depth > 0.0f ? min(min_depth, sample_depth) : sample_depth;
        }
    }

    return min_depth;
}

[numthreads(8, 8, 1)]
void ray_query_main(uint3 dispatch_id : SV_DispatchThreadID)
{
    uint2 launch_size = uint2(scene_uniform.viewport.xy);
    uint2 launch_index = dispatch_id.xy;
    if (launch_index.x >= launch_size.x || launch_index.y >= launch_size.y)
    {
        return;
    }

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

    float depth = sample_min_coarse_depth(pixel_center);
    float coarse_depth_bias = 0.5f;
    if (depth > 0.0f)
    {
        ray.TMin = clamp(depth - coarse_depth_bias, 0.001f, ray.TMax - 0.001f);
    }

    RayData ray_data = trace_voxel_scene(ray);
    float4 debug_color = shade_ray_complexity(ray_data.color, ray_data.step_count);
//     output_image[launch_index] = overlay_fps_counter(launch_index, launch_size, ray_data.color);
    output_image[launch_index] = overlay_fps_counter(launch_index, launch_size, debug_color);
}
