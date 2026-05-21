#include "shared.hlsl"

[shader("closesthit")]
void closest_hit_main(inout RayPayload payload, in HitAttributes attributes)
{
    float3 normal = normalize(attributes.normal);
    float3 light_direction = normalize(float3(0.45f, 0.8f, 0.35f));
    float ndotl = saturate(dot(normal, light_direction));
    float diffuse = 0.15f + ndotl * 0.85f;
    float3 albedo = float3(0.34f, 0.52f, 0.28f);
    float3 color = albedo * diffuse;
    payload.color = float4(color, 1.0f);
}
