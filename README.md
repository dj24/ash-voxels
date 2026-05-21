# ash-voxels

Prototype Vulkan voxel renderer built directly on `ash`, with a small Bevy ECS app layer and HLSL shaders compiled to SPIR-V at build time.

## Repo structure

- `src/main.rs`: tiny binary entry point; just calls `ash_voxels::app::run()`.
- `src/app.rs`: application bootstrap. Owns CLI parsing, the interactive `winit` loop, and the headless screenshot path.
- `src/ecs.rs`: frame state, input state, camera movement, and scene extraction schedule.
- `src/scene.rs`: render-facing scene data types like `Camera`, `SceneUniform`, `RenderObjectData`, and `ExtractedScene`.
- `src/terrain.rs`: procedural terrain model setup and the compute dispatch used to populate voxel occupancy on the GPU.
- `src/assets.rs`: `.vox` loading and conversion into the repo's `VoxelModel` format.
- `src/render/mod.rs`: the renderer and almost all Vulkan-specific code.
- `src/shader_build.rs`: runtime helpers for locating compiled shader artifacts in `OUT_DIR`.
- `src/vk.rs`: shared `AppError` type for Vulkan and app errors.
- `shaders/`: HLSL sources for ray tracing, terrain generation, and coarse depth passes.
- `build.rs`: shader compilation step that invokes `dxc` and emits `.spv` files into Cargo's output directory.
- `tests/shader_artifacts.rs`: checks that the build produced the shader artifacts the renderer expects.
- `plans/`: design notes and experiments.

## App flow

The runtime path is:

1. `src/main.rs` -> `app::run()`
2. `src/app.rs` creates either:
   - an interactive windowed app via `run_interactive()`, or
   - a headless capture app via `run_headless()`
3. both paths build the ECS world with `ecs::create_world()` and the render schedule with `ecs::create_schedule()`
4. each frame:
   - `ecs::begin_frame()` updates timing and viewport state
   - `update_camera()` applies movement input
   - `extract_scene()` packs camera and object data into `ExtractedScene`
   - `Renderer::render()` or `Renderer::render_headless()` submits Vulkan work

## Important functions

- `src/app.rs`
  - `run()`: chooses interactive vs headless mode from CLI args.
  - `AppState::new()`: creates the window, ECS world, schedule, and renderer.
  - `HeadlessAppState::new()`: same idea, but without a window or swapchain.
- `src/ecs.rs`
  - `create_world()`: seeds the ECS world with the camera, resources, and terrain objects.
  - `create_schedule()`: wires the update and extraction systems together.
  - `update_camera()`: WASD + arrows camera controller.
  - `extract_scene()`: produces the compact scene data consumed by the renderer.
- `src/terrain.rs`
  - `procedural_chunk_model()`: defines the voxel chunk dimensions and bounds.
  - `terrain_grid_positions()`: places chunks in a centered grid.
  - `populate_voxel_buffer()`: runs the terrain compute shader through the renderer.
- `src/assets.rs`
  - `VoxelModel::load_from_file()`: loads MagicaVoxel `.vox` assets.
  - `VoxelModel::from_dot_vox_model()`: normalizes voxel data into occupancy + bounds.

## Where the Vulkan boilerplate lives

Most of the Vulkan setup is concentrated in `src/render/mod.rs`.

- `Renderer::new_internal()`: top-level Vulkan bootstrap. Creates the instance, optional debug messenger, surface, device, queues, allocator, command pool, and base renderer resources.
- `pick_physical_device()`, `pick_headless_physical_device()`, `pick_queue_family()`, `pick_graphics_queue_family()`: GPU and queue selection.
- `recreate_swapchain()`: windowed presentation setup and resize handling.
- `create_descriptor_resources()`: descriptor pool, set layout, and descriptor set allocation.
- `create_scene_acceleration()`: BLAS/TLAS setup for ray tracing.
- `create_pipeline_and_sbt()`: ray tracing pipeline creation plus shader binding table setup.
- `create_coarse_depth_shared_resources()` and `recreate_coarse_depth_targets()`: support resources for the coarse depth path.
- `run_compute_shader()`: generic helper used by terrain generation.
- `render()`: frame submission for the windowed renderer.
- `render_headless()` and `save_headless_png()`: offscreen rendering and PNG capture.

If you want to understand the renderer quickly, start with:

1. `Renderer::new_internal()`
2. `Renderer::render()`
3. `Renderer::create_scene_acceleration()`
4. `Renderer::create_pipeline_and_sbt()`

## Shaders

- Sources live in `shaders/`.
- `build.rs` compiles them with `dxc` to SPIR-V during `cargo build`.
- `src/shader_build.rs` exposes helpers like `compiled_shader_artifact()` so the renderer can load the generated `.spv` files at runtime.

## Running

- Interactive: `cargo run`
- Headless screenshot: `cargo run -- --headless-png screenshot-headless.png --delay-ms 1000`
