# Storage Scheme
The goal of the renderer is to compress voxels so more can be rendered on screen, without increasing complexity beyond reason.

Various simplifications will allow for this such as
* All voxels have a consistent size
* All voxels will be axis aligned
* All voxels will exist in world chunks

## Chunks
* The world is chunked into 64x64x64 voxel volumes

### Chunk
* Byte aligned layout
* Packed size: 2,124 bytes

| Byte Size | Stored                    | Notes                                               |
|-----------|---------------------------|-----------------------------------------------------|
| 12        | World position            | `i32 x, y, z` chunk coordinates                     |
| 64        | Region occupancy bitfield | bitfield to indicate if each region contains voxels |
| 2,048     | 512 RegionHeaders         | Info for each region                                |


## Regions
* Each chunk will be divided into 512 8x8x8 palette regions
* Each voxel stores a palette index, with a variable number of bits depending on the size of the palette.
  * For example, a full palette of 255 values would store 8 bits per voxel
  * A palette with 15 distinct values would store 4 bits per voxel
* Initially voxels will be hard capped at 255 variants, but an overflow system could be built later

### RegionHeader
* Byte aligned layout 
* Packed size depends on occupancy and palette size


  | Byte Size | Stored          | Notes                                                   |
  |-----------|-----------------|---------------------------------------------------------|
  | 1         | Palette length  | 1-255 are valid palette sizes, 0 indicates empty region |
 | 3         | Pointer to Blob | 0 indicates empty region                                |

### Blob

 Leaf data

  | Byte Size                                     | Stored           | Notes                                            |
  |-----------------------------------------------|------------------|--------------------------------------------------|
| 2 * `palette_size`                            | Palette swatches |    variable in bit size based on the palette length                                              |
  | `ceil(voxel_count * palette_length_bits / 8)` | Palette indices  | variable in bit size based on the palette length |


### Example Chunk Sizes
Assumptions for the table below:
* Chunk dimensions are `64x64x64` (`262,144` voxels)
* Regions are `8x8x8` (`512` voxels per region, `512` regions per chunk)
* Exactly `2` voxel colours are used, so palette indices cost `1` bit per occupied voxel
* Region headers are already included in the fixed `Chunk` size
* Blob payloads are only allocated for non-empty regions
* `2` colours means each populated region stores `4` bytes of swatches
* With region bitfields removed, a populated `8x8x8` region stores palette indices for all `512` voxels
* At `2` colours, that index payload is `512 bits = 64 bytes` per populated region
* Total bytes = `2124 + populated_regions * 68`

| Chunk Fill | Occupied Voxels | Populated Regions | Total Bytes | Total KiB | Reduction vs 512 KiB dense array |
|------------|-----------------|-------------------|-------------|-----------|----------------------------------|
| 0%         | 0               | 0                 | 2,124       | 2.07      | 99.59%                           |
| 25%        | 65,536          | 128               | 10,828      | 10.57     | 97.93%                           |
| 50%        | 131,072         | 256               | 19,532      | 19.07     | 96.27%                           |
| 100%       | 262,144         | 512               | 36,940      | 36.07     | 92.95%                           |

This table assumes occupied voxels are packed as densely as possible into regions.

For comparison, a dense `2-byte` per voxel array would use `262,144 * 2 = 524,288` bytes (`512 KiB`) per chunk.


### PaletteSwatch
* 2 bytes
* PBR attributes seem overkill, so we can use 3 bits for `voxel_type`, giving 8 variants
* RGB454 is low precision, but can be 4x4x4 bayer (or potentially 8x8x8) dithered for a smoother gradient and retro effect

| Bit Size | Stored     |   
|----------|------------|
| 3        | Voxel Type |
| 4        | Red        |
| 5        | Green      |
| 4        | Blue       |


## Edits

* Large scale edits through player interaction will just apply a list of edits on the CPU side and regenerate the effected chunks from scratch
* Smaller procedural edits like grass and trees fluttering can be done via compute shader, as we will know that it will not expand the palette size 

# Outstanding ideas that need fleshing out

* Solid chunk handling
* Compression algorithm
* Voxel Chunk streaming
* Editing
  * Memory allocation
  * Defrag
* List of voxel types
* Paging
