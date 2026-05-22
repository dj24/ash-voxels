# Lighting system
Once we have emissive voxels marked via the `voxel_type`, we can use those voxels to implicitly light the scene with global illumination.

In a ReGIR-like fashion, we can take advantage of our axis-aligned voxel grid and per-voxel normals for crisp stylized lighting.

## Storage
* 128x128x128 rgba16float texture with 3 mips
* Each mip texel represents a 4x4x4 voxel region, 16x16x16 voxel region, and 64x64x64 voxel region in the world respectively
* Making the max range of GI 4096 voxels in any direction
* Could extend this based on testing


## Visibility Bitfield
* An additional acceleration structure for each mip that is 16x128x128 (8 bits packed on x axis) will store a visibility bitfield, e.g. voxels that were visible that frame
* Primary visiblity pass will mark hit voxels at each mip of the grid, using bitwise atomic image operations
* Clear each frame to ensure we only trace rays from cells visibile that frame

## Sampling
* Check current visiblity bitmask, skip if the bit is 0
* Normal can be computed via driection to camera + neighbouring voxel occupancy (similar to how it's done in dda march)
* Random Hemispherical ray tracing based on the normal
* 1 ray per frame per grid cell
* If ray hits nothing stop
* If ray hits emissive voxel, add that to the radiance and stop
* If ray hits non-emissive voxel:
 1. Add that voxels radiance from the grid ( need to erify this part doesnt just stack sunlight)
 2. shoot a ray towards the sun, add `sun_color * bounced_voxel_color` to the radiance if it doesn't hit any geometry
  

## Compositing
* Blend between mip values based on distance, for a smoother transition

## Temporal Reuse
* The grid will be axis aligned around the player
* Reuse will involve re-snapping the grid based on camera position, if the camera has passed that mip's size threshold
* Instead of full buckets with sample counts, weights, etc, we will use an exponential moving average of the radiance `radiance = mix(new_colour, radiance ,0.95)`
