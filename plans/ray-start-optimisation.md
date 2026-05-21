# Coarse depth as ray start
* To optimise the start position of rays, a cheap depth prepass runs before tracing
* Each voxel chunk is drawn depth-only using conservative rasterisation into a 1/4 size depth buffer
* Mesh shaders can be used to emit vertices from our occupancy array
* The ray gen shader receives this texture as an input
* Sample the t value from the depth texture and convert to world distance to use as the minT value, instead of a hardcoded 0.001

## Phase 1
* Create and render into the depth texture
* Output the depth texture to the screen to visually debug

## Phase 2
* Wire in the depth texture to the ray gen shader
* Set the value from the texture as min t
