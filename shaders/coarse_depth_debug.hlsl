#include "shared.hlsl"

[numthreads(8, 8, 1)]
void coarse_depth_debug_main(uint3 dispatch_id : SV_DispatchThreadID)
{
    uint output_width, output_height;
    output_image.GetDimensions(output_width, output_height);

    if (dispatch_id.x >= output_width || dispatch_id.y >= output_height)
    {
        return;
    }

    float2 uv = (float2(dispatch_id.xy) + 0.5f) / float2(output_width, output_height);
    float depth = coarse_depth_texture.SampleLevel(coarse_depth_sampler, uv, 0.0f);
    float intensity = 0.0f;
    if (depth > 0.0f)
    {
        intensity = saturate(depth / COARSE_DEPTH_FAR);
    }

    output_image[dispatch_id.xy] = float4(intensity, intensity, intensity, 1.0f);
}
