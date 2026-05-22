#include "voxel_hierarchy.hlsl"

bool region_occupancy_at(int3 position, uint3 dimensions, uint instance_id)
{
    if (any(position < 0) || any(position >= int3(dimensions)))
    {
        return false;
    }

    uint3 voxel = uint3(position);
    uint3 region = uint3(voxel.x >> 3, voxel.y >> 3, voxel.z >> 3);
    uint region_index = flatten_region_index(region);
    uint offset = instance_id * CHUNK_OCCUPANCY_WORD_COUNT;
    uint region_word = voxel_occupancy[offset + occupancy_word_index(region_index)];
    return (region_word & occupancy_bit_mask(region_index)) != 0u;
}

uint3 region_grid_dimensions(uint3 voxel_dimensions)
{
    return max((voxel_dimensions + (REGION_AXIS - 1u)) / REGION_AXIS, 1u);
}

bool region_mask_at_region_coord(int3 region_position, uint3 voxel_dimensions, uint instance_id)
{
    uint3 region_dims = region_grid_dimensions(voxel_dimensions);
    if (any(region_position < 0) || any(region_position >= int3(region_dims)))
    {
        return false;
    }

    uint region_index = flatten_region_index(uint3(region_position));
    uint offset = instance_id * CHUNK_OCCUPANCY_WORD_COUNT;
    uint region_word = voxel_occupancy[offset + occupancy_word_index(region_index)];
    return (region_word & occupancy_bit_mask(region_index)) != 0u;
}

float occupancy_at(int3 position, uint3 dimensions, uint instance_id)
{
    if (!region_occupancy_at(position, dimensions, instance_id))
    {
        return 0.0f;
    }

    uint3 voxel = uint3(position);
    uint3 region = uint3(voxel.x >> 3, voxel.y >> 3, voxel.z >> 3);
    uint region_index = flatten_region_index(region);
    uint offset = instance_id * CHUNK_OCCUPANCY_WORD_COUNT;
    uint3 leaf_local = uint3(voxel.x & 7u, voxel.y & 7u, voxel.z & 7u);
    uint leaf_index = flatten_leaf_index(leaf_local);
    uint leaf_word = voxel_occupancy[
        offset + leaf_mask_word_offset(region_index) + occupancy_word_index(leaf_index)];
    return (leaf_word & occupancy_bit_mask(leaf_index)) == 0u ? 0.0f : 1.0f;
}

bool ray_box(
    float3 origin,
    float3 direction,
    float3 bounds_min,
    float3 bounds_max,
    float ray_t_min,
    float ray_t_max,
    out float t_enter,
    out float t_exit);

bool enter_voxel_object(
    float3 origin,
    float3 direction,
    float ray_t_min,
    float ray_t_max,
    RenderObjectData object,
    out float3 bounds_min,
    out float3 bounds_max,
    out float voxel_size,
    out uint3 grid_dims,
    out float3 extent,
    out float t_enter,
    out float t_exit)
{
    bounds_min = object.bounds_min.xyz;
    bounds_max = object.bounds_max.xyz;
    voxel_size = object.voxel_size_and_dimensions.x;
    grid_dims = uint3(
        max(1, (int)object.voxel_size_and_dimensions.y),
        max(1, (int)object.voxel_size_and_dimensions.z),
        max(1, (int)object.voxel_size_and_dimensions.w));
    extent = bounds_max - bounds_min;
    return ray_box(origin, direction, bounds_min, bounds_max, ray_t_min, ray_t_max, t_enter, t_exit);
}

int3 initial_grid_cell(
    float3 origin,
    float3 direction,
    float t_enter,
    float3 bounds_min,
    float3 bounds_max,
    float3 extent,
    uint3 grid_dims)
{
    float3 local_point = clamp(origin + direction * t_enter, bounds_min, bounds_max - 1e-4f);
    float3 relative = saturate(
        (local_point - bounds_min) / max(extent, float3(1e-5f, 1e-5f, 1e-5f)));
    return min(int3(relative * float3(grid_dims)), int3(grid_dims) - 1);
}

void rebuild_dda_state(
    float3 origin,
    float3 direction,
    float3 bounds_min,
    float voxel_size,
    int3 cell,
    out float3 t_max)
{
    float3 next_boundary = bounds_min + (float3(cell) + float3(direction >= 0.0f)) * voxel_size;
    t_max = float3(
        abs(direction.x) > 1e-5f ? (next_boundary.x - origin.x) / direction.x : 1e30f,
        abs(direction.y) > 1e-5f ? (next_boundary.y - origin.y) / direction.y : 1e30f,
        abs(direction.z) > 1e-5f ? (next_boundary.z - origin.z) / direction.z : 1e30f);
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

bool intersect_occupied_region_object(
    float3 origin,
    float3 direction,
    float ray_t_min,
    float ray_t_max,
    RenderObjectData object,
    uint instance_id,
    out float hit_t,
    out uint step_count)
{
    float3 bounds_min;
    float3 bounds_max;
    float voxel_size;
    uint3 grid_dims;
    float3 extent;
    float t_enter;
    float t_exit;
    step_count = 0u;

    if (!enter_voxel_object(
        origin,
        direction,
        ray_t_min,
        ray_t_max,
        object,
        bounds_min,
        bounds_max,
        voxel_size,
        grid_dims,
        extent,
        t_enter,
        t_exit))
    {
        hit_t = 0.0f;
        return false;
    }

    uint3 region_dims = region_grid_dimensions(grid_dims);
    float region_size = voxel_size * (float)REGION_AXIS;
    int3 cell = initial_grid_cell(
        origin,
        direction,
        t_enter,
        bounds_min,
        bounds_max,
        extent,
        region_dims);

    int3 step_dir = int3(
        direction.x >= 0.0f ? 1 : -1,
        direction.y >= 0.0f ? 1 : -1,
        direction.z >= 0.0f ? 1 : -1);

    float3 t_max;
    rebuild_dda_state(origin, direction, bounds_min, region_size, cell, t_max);
    float3 t_delta = abs(region_size / max(abs(direction), 1e-5f));

    [loop]
    for (uint step_index = 0; step_index < REGION_COUNT; ++step_index)
    {
        if (any(cell < 0) || any(cell >= int3(region_dims)))
        {
            break;
        }

        step_count += 1u;
        if (region_mask_at_region_coord(cell, grid_dims, instance_id))
        {
            hit_t = max(t_enter, ray_t_min);
            return true;
        }

        if (t_max.x < t_max.y && t_max.x < t_max.z)
        {
            t_enter = t_max.x;
            t_max.x += t_delta.x;
            cell.x += step_dir.x;
        }
        else if (t_max.y < t_max.z)
        {
            t_enter = t_max.y;
            t_max.y += t_delta.y;
            cell.y += step_dir.y;
        }
        else
        {
            t_enter = t_max.z;
            t_max.z += t_delta.z;
            cell.z += step_dir.z;
        }

        if (t_enter > t_exit || t_enter > ray_t_max)
        {
            break;
        }
    }

    hit_t = 0.0f;
    return false;
}

bool intersect_voxel_object(
    float3 origin,
    float3 direction,
    float ray_t_min,
    float ray_t_max,
    RenderObjectData object,
    uint instance_id,
    out float hit_t,
    out float3 hit_normal,
    out uint step_count)
{
    float3 bounds_min;
    float3 bounds_max;
    float voxel_size;
    uint3 grid_dims;
    float3 extent;
    float t_enter;
    float t_exit;
    step_count = 0u;
    if (!enter_voxel_object(
        origin,
        direction,
        ray_t_min,
        ray_t_max,
        object,
        bounds_min,
        bounds_max,
        voxel_size,
        grid_dims,
        extent,
        t_enter,
        t_exit))
    {
        hit_t = 0.0f;
        hit_normal = float3(0.0f, 1.0f, 0.0f);
        return false;
    }

    int3 cell = initial_grid_cell(
        origin,
        direction,
        t_enter,
        bounds_min,
        bounds_max,
        extent,
        grid_dims);

    int3 step_dir = int3(
        direction.x >= 0.0f ? 1 : -1,
        direction.y >= 0.0f ? 1 : -1,
        direction.z >= 0.0f ? 1 : -1);

    float3 t_max;
    rebuild_dda_state(origin, direction, bounds_min, voxel_size, cell, t_max);
    float3 t_delta = abs(voxel_size / max(abs(direction), 1e-5f));
    int last_axis = -1;
    float advance_epsilon = voxel_size * 0.5f;

    [loop]
    for (uint step_index = 0; step_index < 512; ++step_index)
    {
        if (any(cell < 0) || any(cell >= int3(grid_dims)))
        {
            break;
        }

        step_count += 1u;
        if (!region_occupancy_at(cell, grid_dims, instance_id))
        {
            int3 region = int3(cell.x >> 3, cell.y >> 3, cell.z >> 3);
            int3 region_local = int3(cell.x & 7, cell.y & 7, cell.z & 7);
            int3 steps_to_region_exit = int3(
                step_dir.x > 0 ? 8 - region_local.x : region_local.x + 1,
                step_dir.y > 0 ? 8 - region_local.y : region_local.y + 1,
                step_dir.z > 0 ? 8 - region_local.z : region_local.z + 1);

            float3 t_region_exit = t_max + float3(
                (steps_to_region_exit.x - 1) * t_delta.x,
                (steps_to_region_exit.y - 1) * t_delta.y,
                (steps_to_region_exit.z - 1) * t_delta.z);

            if (t_region_exit.x < t_region_exit.y && t_region_exit.x < t_region_exit.z)
            {
                t_enter = t_region_exit.x;
                last_axis = 0;
            }
            else if (t_region_exit.y < t_region_exit.z)
            {
                t_enter = t_region_exit.y;
                last_axis = 1;
            }
            else
            {
                t_enter = t_region_exit.z;
                last_axis = 2;
            }

            if (t_enter > t_exit || t_enter > ray_t_max)
            {
                break;
            }

            float travel_t = min(t_enter + advance_epsilon, t_exit);
            float3 advanced_point = clamp(
                origin + direction * travel_t,
                bounds_min,
                bounds_max - 1e-4f);
            float3 advanced_relative = saturate(
                (advanced_point - bounds_min) / max(extent, float3(1e-5f, 1e-5f, 1e-5f)));
            cell = min(int3(advanced_relative * float3(grid_dims)), int3(grid_dims) - 1);
            if (all(region == int3(cell.x >> 3, cell.y >> 3, cell.z >> 3)))
            {
                if (last_axis == 0)
                {
                    cell.x += step_dir.x * steps_to_region_exit.x;
                }
                else if (last_axis == 1)
                {
                    cell.y += step_dir.y * steps_to_region_exit.y;
                }
                else
                {
                    cell.z += step_dir.z * steps_to_region_exit.z;
                }
            }
            rebuild_dda_state(origin, direction, bounds_min, voxel_size, cell, t_max);
            continue;
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
