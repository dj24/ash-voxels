#include "shared.hlsl"

[shader("miss")]
void miss_main(inout RayPayload payload)
{
    float3 direction = WorldRayDirection();
    float sky = 0.5f * (direction.y + 1.0f);
    float3 color = lerp(float3(0.12f, 0.15f, 0.2f), float3(0.65f, 0.75f, 0.95f), sky);
    payload.color = float4(color, 1.0f);
}
