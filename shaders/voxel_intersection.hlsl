#include "voxel_hierarchy.hlsl"

uint3 mip8_cell_from_voxel(uint3 voxel)
{
    return voxel >> 5;
}

uint3 mip64_cell_from_voxel(uint3 voxel)
{
    return voxel >> 4;
}

uint3 region_cell_from_voxel(uint3 voxel)
{
    return voxel >> 3;
}

bool mip8_occupancy_at(int3 position, uint3 dimensions, uint instance_id)
{
    if (any(position < 0) || any(position >= int3(dimensions)))
    {
        return false;
    }

    uint3 voxel = uint3(position);
    uint mip8_index = flatten_mip8_region_index(mip8_cell_from_voxel(voxel));
    uint offset = instance_id * CHUNK_OCCUPANCY_WORD_COUNT;
    return mip8_mask_bit_is_set(offset, mip8_index);
}

bool mip64_occupancy_at(int3 position, uint3 dimensions, uint instance_id)
{
    if (any(position < 0) || any(position >= int3(dimensions)))
    {
        return false;
    }

    uint3 voxel = uint3(position);
    uint mip64_index = flatten_mip64_region_index(mip64_cell_from_voxel(voxel));
    uint offset = instance_id * CHUNK_OCCUPANCY_WORD_COUNT;
    return mip64_mask_bit_is_set(offset, mip64_index);
}

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

bool skip_empty_coarse_cell(
    int3 cell,
    int3 step_dir,
    float3 t_max,
    float3 t_delta,
    uint span,
    float3 origin,
    float3 direction,
    float3 bounds_min,
    float3 bounds_max,
    float3 extent,
    float voxel_size,
    uint3 grid_dims,
    float t_exit,
    float ray_t_max,
    float advance_epsilon,
    out int3 updated_cell,
    out float updated_t_enter,
    out int updated_last_axis,
    out float3 updated_t_max)
{
    int3 local = int3(cell.x & int(span - 1u), cell.y & int(span - 1u), cell.z & int(span - 1u));
    int3 steps_to_exit = int3(
        step_dir.x > 0 ? int(span) - local.x : local.x + 1,
        step_dir.y > 0 ? int(span) - local.y : local.y + 1,
        step_dir.z > 0 ? int(span) - local.z : local.z + 1);
    float3 t_cell_exit = t_max + float3(
        (steps_to_exit.x - 1) * t_delta.x,
        (steps_to_exit.y - 1) * t_delta.y,
        (steps_to_exit.z - 1) * t_delta.z);

    int last_axis;
    float t_enter;
    if (t_cell_exit.x < t_cell_exit.y && t_cell_exit.x < t_cell_exit.z)
    {
        t_enter = t_cell_exit.x;
        last_axis = 0;
    }
    else if (t_cell_exit.y < t_cell_exit.z)
    {
        t_enter = t_cell_exit.y;
        last_axis = 1;
    }
    else
    {
        t_enter = t_cell_exit.z;
        last_axis = 2;
    }

    if (t_enter > t_exit || t_enter > ray_t_max)
    {
        return false;
    }

    int3 coarse_cell = cell / int(span);
    float travel_t = min(t_enter + advance_epsilon, t_exit);
    float3 advanced_point = clamp(origin + direction * travel_t, bounds_min, bounds_max - 1e-4f);
    float3 advanced_relative = saturate(
        (advanced_point - bounds_min) / max(extent, float3(1e-5f, 1e-5f, 1e-5f)));
    int3 next_cell = min(int3(advanced_relative * float3(grid_dims)), int3(grid_dims) - 1);
    if (all(coarse_cell == (next_cell / int(span))))
    {
        if (last_axis == 0)
        {
            next_cell.x += step_dir.x * steps_to_exit.x;
        }
        else if (last_axis == 1)
        {
            next_cell.y += step_dir.y * steps_to_exit.y;
        }
        else
        {
            next_cell.z += step_dir.z * steps_to_exit.z;
        }
    }

    updated_cell = next_cell;
    updated_t_enter = t_enter;
    updated_last_axis = last_axis;
    rebuild_dda_state(origin, direction, bounds_min, voxel_size, updated_cell, updated_t_max);
    return true;
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
        int3 skipped_cell;
        float skipped_t_enter;
        int skipped_last_axis;
        float3 skipped_t_max;
        if (!mip8_occupancy_at(cell, grid_dims, instance_id)
            && skip_empty_coarse_cell(
                cell,
                step_dir,
                t_max,
                t_delta,
                32u,
                origin,
                direction,
                bounds_min,
                bounds_max,
                extent,
                voxel_size,
                grid_dims,
                t_exit,
                ray_t_max,
                advance_epsilon,
                skipped_cell,
                skipped_t_enter,
                skipped_last_axis,
                skipped_t_max))
        {
            cell = skipped_cell;
            t_enter = skipped_t_enter;
            last_axis = skipped_last_axis;
            t_max = skipped_t_max;
            continue;
        }

        if (!mip64_occupancy_at(cell, grid_dims, instance_id)
            && skip_empty_coarse_cell(
                cell,
                step_dir,
                t_max,
                t_delta,
                16u,
                origin,
                direction,
                bounds_min,
                bounds_max,
                extent,
                voxel_size,
                grid_dims,
                t_exit,
                ray_t_max,
                advance_epsilon,
                skipped_cell,
                skipped_t_enter,
                skipped_last_axis,
                skipped_t_max))
        {
            cell = skipped_cell;
            t_enter = skipped_t_enter;
            last_axis = skipped_last_axis;
            t_max = skipped_t_max;
            continue;
        }

        if (!region_occupancy_at(cell, grid_dims, instance_id)
            && skip_empty_coarse_cell(
                cell,
                step_dir,
                t_max,
                t_delta,
                8u,
                origin,
                direction,
                bounds_min,
                bounds_max,
                extent,
                voxel_size,
                grid_dims,
                t_exit,
                ray_t_max,
                advance_epsilon,
                skipped_cell,
                skipped_t_enter,
                skipped_last_axis,
                skipped_t_max))
        {
            cell = skipped_cell;
            t_enter = skipped_t_enter;
            last_axis = skipped_last_axis;
            t_max = skipped_t_max;
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
