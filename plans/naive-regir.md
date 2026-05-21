# Lighting system
Once we have emissive voxels marked via the `voxel_type`, we can use those voxels to implicitly light the scene with global illumination.

In a ReGIR-like fashion, we can take advantage of our axis-aligned voxel grid and per-voxel normals for crisp stylized lighting.

## Storage
* 128x128x128 rgba16float texture with 3 mips
* Each mip represents the scene at 1 voxel, 4x voxel, and 16x voxel scale respectively
* Making the max range of GI 1024 voxels in any direction
* Could extend this based on testing

## Sampling
* Random Hemispherical ray tracing based on the per-voxel normal derived in DDA raymarch
* 1 ray per frame per grid cell
* Path guiding _could_ still be used, but for now we will naively fire rays in random directions
* Before tracing a cell, traverse the voxel structure to skip tracing empty voxel positions
* Blend between mip values based on distance, for a smoother transition

## Temporal Reuse
* The grid will be axis aligned around the player
* Reuse will involve re-snapping the grid based on camera position, if the camera has passed that mip's size threshold
* Instead of full buckets with sample counts, weights, etc, we will use an exponential moving average of the radiance `radiance = mix(radiance, new_colour ,0.95)`

## Outstanding Issues
* How can we get normals in the grid without tracing rays from the camera?
  * Voxel normals are derived from DDA step direction, so we will need to rtrace rays or use the direction to the camera to approximate
* Should we construct conservative lower LOD versions of the voxels to limit light leaking at coarser levels?
