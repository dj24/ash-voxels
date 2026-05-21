#include "shared.hlsl"

[shader("closesthit")]
void closest_hit_main(inout RayPayload payload, in HitAttributes attributes)
{
    float3 normal = normalize(attributes.normal);
    float3 color = normal * 0.5f + 0.5f;
    payload.color = float4(normal, 1.0f);
}
