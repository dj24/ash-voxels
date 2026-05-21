# Storage Scheme
The goal of the renderer is to compress voxels so more can be rendered on screen, without increasing complexity beyond reason.

Various simplifications will allow for this such as
* All voxels have a consistent size
* All voxels will be axis aligned
* All voxels will exist in world chunks

## Chunks
* The world is chunked into 64x64x64 voxel volumes

### ChunkHeader
| Bit Size                       | Stored                  |   
|--------------------------------|-------------------------|
| 512                            | Occupancy Bitmask       |
| 512 * `size_of(PaletteHeader)` | List of palette headers |
| 32 * 3                         | World position          |


## Palette Regions
* Each chunk will be divided into 512 8x8x8 palette regions
* Each voxel stores a palette index, with a variable number of bits depending on the size of the palette.
  * For example, a full palette of 255 values would store 8 bits per voxel
  * A palette with 15 distinct values would store 4 bits per voxel

### PaletteHeader
  | Bit Size | Stored                                        |   
  |----------|-----------------------------------------------|
  | 512      | Occupancy Bitmask                             |
  | 8        | Palette length (Could change if too limiting) |
  | 32       | Start index in the voxel buffer               |


### Swatch
| Bit Size | Stored     |   
|---------|------------|
| 4       | Voxel Type |
| 4       | Red        |
| 4       | Green      |
| 4       | Blue       |


### Voxels
| Bit Size                            | Stored                                                            |   
|-------------------------------------|-------------------------------------------------------------------|
| `voxel_count * palette_length_bits` | Palette indices, variable in bit size based on the palette length |


# Outstanding ideas that need fleshing out

* Per voxel secondary ray tracing
  * Shadows, GI, etc
* Voxel Chunk streaming
* Editing