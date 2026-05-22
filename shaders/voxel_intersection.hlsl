uint flatten_index(uint3 position, uint3 dimensions)
{
    return position.x + dimensions.x * (position.y + dimensions.y * position.z);
}

uint chunk_voxel_count(uint3 dimensions)
{
    return dimensions.x * dimensions.y * dimensions.z;
}

uint chunk_occupancy_word_count(uint3 dimensions)
{
    return (chunk_voxel_count(dimensions) + OCCUPANCY_WORD_BITS - 1u) / OCCUPANCY_WORD_BITS;
}

uint object_voxel_word_offset(uint instance_id, uint3 dimensions)
{
    return instance_id * chunk_occupancy_word_count(dimensions);
}

float occupancy_at(int3 position, uint3 dimensions, uint instance_id)
{
    if (any(position < 0) || any(position >= int3(dimensions)))
    {
        return 0.0f;
    }

    uint voxel_index = flatten_index(uint3(position), dimensions);
    uint offset = object_voxel_word_offset(instance_id, dimensions);
    uint word_index = voxel_index / OCCUPANCY_WORD_BITS;
    uint bit_index = voxel_index % OCCUPANCY_WORD_BITS;
    uint word = voxel_occupancy[offset + word_index];
    return ((word >> bit_index) & 1u) == 0u ? 0.0f : 1.0f;
}

float3 fallback_normal(float3 direction, int last_axis, int3 step_dir)
{
    if (last_axis == 0)
    {
        return float3(-step_dir.x, 0.0f, 0.0f);
    }
    if (last_axis == 1)
    {
        return float3(0.0f, -step_dir.y, 0.0f);
    }
    if (last_axis == 2)
    {
        return float3(0.0f, 0.0f, -step_dir.z);
    }

    float3 axis = abs(direction);
    if (axis.x >= axis.y && axis.x >= axis.z)
    {
        return float3(direction.x >= 0.0f ? -1.0f : 1.0f, 0.0f, 0.0f);
    }
    if (axis.y >= axis.z)
    {
        return float3(0.0f, direction.y >= 0.0f ? -1.0f : 1.0f, 0.0f);
    }
    return float3(0.0f, 0.0f, direction.z >= 0.0f ? -1.0f : 1.0f);
}

bool ray_box(
    float3 origin,
    float3 direction,
    float3 bounds_min,
    float3 bounds_max,
    float ray_t_min,
    float ray_t_max,
    out float t_enter,
    out float t_exit)
{
    float3 inv_dir = 1.0f / direction;
    float3 t0 = (bounds_min - origin) * inv_dir;
    float3 t1 = (bounds_max - origin) * inv_dir;
    float3 tmin = min(t0, t1);
    float3 tmax = max(t0, t1);
    t_enter = max(max(tmin.x, tmin.y), max(tmin.z, ray_t_min));
    t_exit = min(min(tmax.x, tmax.y), min(tmax.z, ray_t_max));
    return t_exit >= t_enter;
}

bool intersect_voxel_object(
    float3 origin,
    float3 direction,
    float ray_t_min,
    float ray_t_max,
    RenderObjectData object,
    uint instance_id,
    out float hit_t,
    out float3 hit_normal)
{
    float3 bounds_min = object.bounds_min.xyz;
    float3 bounds_max = object.bounds_max.xyz;
    float voxel_size = object.voxel_size_and_dimensions.x;
    uint3 grid_dims = uint3(
        max(1, (int)object.voxel_size_and_dimensions.y),
        max(1, (int)object.voxel_size_and_dimensions.z),
        max(1, (int)object.voxel_size_and_dimensions.w));

    float t_enter;
    float t_exit;
    if (!ray_box(origin, direction, bounds_min, bounds_max, ray_t_min, ray_t_max, t_enter, t_exit))
    {
        hit_t = 0.0f;
        hit_normal = float3(0.0f, 1.0f, 0.0f);
        return false;
    }

    float3 extent = bounds_max - bounds_min;
    float3 local_point = clamp(origin + direction * t_enter, bounds_min, bounds_max - 1e-4f);
    float3 relative = saturate((local_point - bounds_min) / max(extent, float3(1e-5f, 1e-5f, 1e-5f)));
    int3 cell = min(int3(relative * float3(grid_dims)), int3(grid_dims) - 1);

    int3 step_dir = int3(
        direction.x >= 0.0f ? 1 : -1,
        direction.y >= 0.0f ? 1 : -1,
        direction.z >= 0.0f ? 1 : -1);

    float3 next_boundary = bounds_min + (float3(cell) + float3(step_dir > 0)) * voxel_size;
    float3 t_max = float3(
        abs(direction.x) > 1e-5f ? (next_boundary.x - origin.x) / direction.x : 1e30f,
        abs(direction.y) > 1e-5f ? (next_boundary.y - origin.y) / direction.y : 1e30f,
        abs(direction.z) > 1e-5f ? (next_boundary.z - origin.z) / direction.z : 1e30f);
    float3 t_delta = abs(voxel_size / max(abs(direction), 1e-5f));
    int last_axis = -1;

    [loop]
    for (uint step_index = 0; step_index < 512; ++step_index)
    {
        if (any(cell < 0) || any(cell >= int3(grid_dims)))
        {
            break;
        }

        if (occupancy_at(cell, grid_dims, instance_id) > 0.5f)
        {
            float3 gradient = float3(
                occupancy_at(cell + int3(-1, 0, 0), grid_dims, instance_id) - occupancy_at(cell + int3(1, 0, 0), grid_dims, instance_id),
                occupancy_at(cell + int3(0, -1, 0), grid_dims, instance_id) - occupancy_at(cell + int3(0, 1, 0), grid_dims, instance_id),
                occupancy_at(cell + int3(0, 0, -1), grid_dims, instance_id) - occupancy_at(cell + int3(0, 0, 1), grid_dims, instance_id));

            hit_t = max(t_enter, ray_t_min);
            hit_normal = length(gradient) > 1e-5f
                ? normalize(gradient)
                : fallback_normal(direction, last_axis, step_dir);
            return true;
        }

        if (t_max.x < t_max.y && t_max.x < t_max.z)
        {
            t_enter = t_max.x;
            t_max.x += t_delta.x;
            cell.x += step_dir.x;
            last_axis = 0;
        }
        else if (t_max.y < t_max.z)
        {
            t_enter = t_max.y;
            t_max.y += t_delta.y;
            cell.y += step_dir.y;
            last_axis = 1;
        }
        else
        {
            t_enter = t_max.z;
            t_max.z += t_delta.z;
            cell.z += step_dir.z;
            last_axis = 2;
        }

        if (t_enter > t_exit || t_enter > ray_t_max)
        {
            break;
        }
    }

    hit_t = 0.0f;
    hit_normal = float3(0.0f, 1.0f, 0.0f);
    return false;
}
