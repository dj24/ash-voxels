# Coarse depth as ray start
* To optimise the start position of rays, a cheap depth prepass runs before tracing
* Each voxel chunk is ray traced at 1/8 resolution, using the region occupancy bitfield only
* The ray gen shader receives this texture as an input
* Ray gen shader samples min depth over a 3×3 low-res neighbourhood.
* Convert to world distance to use as the minT value, instead of a hardcoded 0.001

## Phase 1
* Create and render into the depth texture
* Output the depth texture to the screen to visually debug

## Phase 2
* Wire in the depth texture to the ray gen shader
* Set the value from the texture as min t
