# Storage Scheme
The goal of the renderer is to compress voxels so more can be rendered on screen, without increasing complexity beyond reason.

Various simplifications will allow for this such as
* All voxels have a consistent size
* All voxels will be axis aligned
* All voxels will exist in world chunks

## Chunks
* The world is chunked into 64x64x64 voxel volumes

### ChunkHeader
* Byte aligned layout
* Packed size: 33,408 bytes

| Byte Size | Stored                  | Notes                            |
|-----------|-------------------------|----------------------------------|
| 64        | Occupancy Bitmask       | 512 bits, 1 bit per palette region |
| 33,280    | List of palette headers | 512 headers, 65 bytes each       |
| 12        | World position          | `i32 x, y, z` chunk coordinates  |
| 52        | Reserved / padding      | Keeps the header 64-byte aligned |


## Palette Regions
* Each chunk will be divided into 512 8x8x8 palette regions
* Each voxel stores a palette index, with a variable number of bits depending on the size of the palette.
  * For example, a full palette of 255 values would store 8 bits per voxel
  * A palette with 15 distinct values would store 4 bits per voxel
* Initially voxels will be hard capped at 255 variants, but an overflow system could be built later

### PaletteHeader
* Byte aligned layout
* Packed size: 65 bytes

  | Byte Size | Stored                  | Notes                                              |
  |-----------|-------------------------|----------------------------------------------------|
  | 64        | Occupancy Bitmask       | 512 bits, 1 bit per voxel in the `8x8x8` region   |
  | 1         | Palette length          | 0 means empty region, 1-255 are valid palette sizes |

* `PaletteHeader` does not store the voxel buffer start index inline.
* Region payload offsets should live in a separate `u32` offset table so the fixed-size header stays compact and byte aligned.
* If inline offsets are preferred instead, round the header up to 16 bytes and use:

  | Byte Size | Stored                  | Notes                                 |
  |-----------|-------------------------|---------------------------------------|
  | 64        | Occupancy Bitmask       | 512 bits, 1 bit per voxel             |
  | 1         | Palette length          | 0 for empty                           |
  | 3         | Padding                 | Aligns the following offset to 4 bytes |
  | 4         | Voxel buffer start index | Offset into the packed voxel payload |


### Swatch
* 2 bytes
* PBR attributes seem overkill, so we can use 3 bits for `voxel_type`, giving 8 variants
* RGB454 is low precision, but can be 4x4x4 bayer (or potentially 8x8x8) dithered for a smoother gradient and retro effect 

| Bit Size | Stored     |   
|----------|------------|
| 3        | Voxel Type |
| 4        | Red        |
| 5        | Green      |
| 4        | Blue       |


### Voxels
* Byte aligned payload stream
* Each palette region payload should be padded to a whole byte after bit-packing indices.

| Byte Size                                 | Stored                                                            |
|-------------------------------------------|-------------------------------------------------------------------|
| `ceil(voxel_count * palette_length_bits / 8)` | Palette indices, variable in bit size based on the palette length |

## Offset Tables
* Store palette swatches and voxel payloads in separate append-only buffers.
* Use fixed-width offset tables for GPU-friendly addressing.

### RegionOffsetTable
| Byte Size | Stored                    | Notes                                   |
|-----------|---------------------------|-----------------------------------------|
| 2,048     | 512 `u32` region offsets  | One payload offset per palette region   |

### RegionPaletteTable
| Byte Size | Stored                    | Notes                                 |
|-----------|---------------------------|---------------------------------------|
| Variable  | Palette swatches          | `palette_len * 2` bytes per populated region |

## Edits

* Large scale edits through player interaction will just apply a list of edits on the CPU side and regenerate the effected chunks from scratch
* Smaller procedural edits like grass and trees fluttering can be done via compute shader, as we will know that it will not expand the palette size 

# Outstanding ideas that need fleshing out

* Per voxel secondary ray tracing
  * Shadows, GI, etc
* Voxel Chunk streaming
* Editing
  * Memory allocation
  * Defrag
* List of voxel types
