use std::{
    borrow::Cow,
    ffi::{CStr, CString, c_void},
    io::Cursor,
    mem::{align_of, size_of},
    path::Path,
};

use ash::{
    Device, Entry, Instance,
    util::read_spv,
    vk::{self},
};
use bevy_ecs::prelude::Resource;
use gpu_allocator::{
    AllocationSizes, MemoryLocation,
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc},
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use tracing::{info, warn};

use crate::{
    assets::VoxelModel,
    scene::{
        ExtractedScene, RenderObjectData, TERRAIN_GRID_COUNT, VoxelProceduralObject,
        terrain_grid_positions,
    },
    shader_build::{compiled_shader_artifact, compiled_shader_artifacts},
    vk::AppError,
};

const MAX_OBJECTS: usize = TERRAIN_GRID_COUNT;
const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";
const WINDOWED_DEVICE_EXTENSIONS: &[&CStr] = &[
    ash::khr::swapchain::NAME,
    ash::khr::acceleration_structure::NAME,
    ash::khr::ray_tracing_pipeline::NAME,
    ash::khr::ray_query::NAME,
    ash::khr::deferred_host_operations::NAME,
    ash::khr::buffer_device_address::NAME,
    ash::ext::descriptor_indexing::NAME,
    ash::khr::spirv_1_4::NAME,
    ash::khr::shader_float_controls::NAME,
];
const HEADLESS_DEVICE_EXTENSIONS: &[&CStr] = &[
    ash::khr::acceleration_structure::NAME,
    ash::khr::ray_tracing_pipeline::NAME,
    ash::khr::ray_query::NAME,
    ash::khr::deferred_host_operations::NAME,
    ash::khr::buffer_device_address::NAME,
    ash::ext::descriptor_indexing::NAME,
    ash::khr::spirv_1_4::NAME,
    ash::khr::shader_float_controls::NAME,
];
const HEADLESS_CAPTURE_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
const HEADLESS_OUTPUT_FORMAT: vk::Format = vk::Format::R32G32B32A32_SFLOAT;

#[derive(Clone, Debug, Resource)]
pub struct RenderDeviceCaps {
    pub device_name: String,
    pub shader_group_handle_size: u32,
    pub shader_group_base_alignment: u32,
    pub max_ray_recursion_depth: u32,
}

pub struct Renderer {
    _entry: Entry,
    instance: Instance,
    debug_utils: Option<ash::ext::debug_utils::Instance>,
    debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
    surface_loader: Option<ash::khr::surface::Instance>,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: Device,
    graphics_queue: vk::Queue,
    swapchain_loader: Option<ash::khr::swapchain::Device>,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_initialized: Vec<bool>,
    swapchain_format: vk::SurfaceFormatKHR,
    extent: vk::Extent2D,
    allocator: Option<Allocator>,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: Vec<vk::Semaphore>,
    in_flight: vk::Fence,
    output_image: AllocatedImage,
    capture_image: AllocatedImage,
    readback_buffer: AllocatedBuffer,
    uniform_buffer: AllocatedBuffer,
    object_buffer: AllocatedBuffer,
    voxel_buffer: AllocatedBuffer,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_set: vk::DescriptorSet,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    sbt: ShaderBindingTable,
    acceleration_loader: ash::khr::acceleration_structure::Device,
    ray_tracing_pipeline_loader: ash::khr::ray_tracing_pipeline::Device,
    scene_acceleration: SceneAcceleration,
    device_caps: RenderDeviceCaps,
    mode: RendererMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RendererMode {
    Windowed,
    Headless,
}

impl Renderer {
    pub fn new(
        _window: &winit::window::Window,
        display_handle: RawDisplayHandle,
        window_handle: RawWindowHandle,
        initial_size: [u32; 2],
        model: &VoxelModel,
    ) -> Result<Self, AppError> {
        Self::new_internal(
            RendererMode::Windowed,
            Some((display_handle, window_handle)),
            initial_size,
            model,
        )
    }

    pub fn new_headless(initial_size: [u32; 2], model: &VoxelModel) -> Result<Self, AppError> {
        Self::new_internal(RendererMode::Headless, None, initial_size, model)
    }

    fn new_internal(
        mode: RendererMode,
        window_handles: Option<(RawDisplayHandle, RawWindowHandle)>,
        initial_size: [u32; 2],
        model: &VoxelModel,
    ) -> Result<Self, AppError> {
        let entry =
            unsafe { Entry::load() }.map_err(|error| AppError::Message(error.to_string()))?;

        let available_layers = unsafe { entry.enumerate_instance_layer_properties()? };
        let enable_validation = cfg!(debug_assertions)
            && available_layers.iter().any(|layer| unsafe {
                CStr::from_ptr(layer.layer_name.as_ptr()) == VALIDATION_LAYER
            });

        let mut extension_names = match window_handles {
            Some((display_handle, _)) => {
                ash_window::enumerate_required_extensions(display_handle)?.to_vec()
            }
            None => Vec::new(),
        };
        if enable_validation {
            extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
        }

        let app_name = CString::new("ash-voxels")?;
        let engine_name = CString::new("ash")?;
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&engine_name)
            .engine_version(0)
            .api_version(vk::API_VERSION_1_3);

        let mut layer_names = Vec::new();
        if enable_validation {
            layer_names.push(VALIDATION_LAYER.as_ptr());
        }

        let instance_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extension_names)
            .enabled_layer_names(&layer_names);

        let instance = unsafe { entry.create_instance(&instance_info, None)? };
        let debug_utils =
            enable_validation.then(|| ash::ext::debug_utils::Instance::new(&entry, &instance));
        let debug_messenger = debug_utils
            .as_ref()
            .map(|loader| {
                let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                            | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                    )
                    .message_type(
                        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    )
                    .pfn_user_callback(Some(debug_callback));

                unsafe { loader.create_debug_utils_messenger(&info, None) }
            })
            .transpose()?;

        let (surface_loader, surface) = match window_handles {
            Some((display_handle, window_handle)) => {
                let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
                let surface = unsafe {
                    ash_window::create_surface(
                        &entry,
                        &instance,
                        display_handle,
                        window_handle,
                        None,
                    )?
                };
                (Some(surface_loader), surface)
            }
            None => (None, vk::SurfaceKHR::null()),
        };

        let physical_device = match mode {
            RendererMode::Windowed => pick_physical_device(
                &instance,
                surface_loader
                    .as_ref()
                    .expect("windowed renderer should create a surface loader"),
                surface,
                WINDOWED_DEVICE_EXTENSIONS,
            )?,
            RendererMode::Headless => pick_headless_physical_device(&instance)?,
        };
        let queue_family_index = match mode {
            RendererMode::Windowed => pick_queue_family(
                &instance,
                surface_loader
                    .as_ref()
                    .expect("windowed renderer should create a surface loader"),
                physical_device,
                surface,
            )?,
            RendererMode::Headless => pick_graphics_queue_family(&instance, physical_device)?,
        };

        let device_extension_names = match mode {
            RendererMode::Windowed => WINDOWED_DEVICE_EXTENSIONS,
            RendererMode::Headless => HEADLESS_DEVICE_EXTENSIONS,
        };
        let device_extensions: Vec<*const i8> = device_extension_names
            .iter()
            .map(|name| name.as_ptr())
            .collect();
        let priorities = [1.0f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)];

        let mut descriptor_indexing_features =
            vk::PhysicalDeviceDescriptorIndexingFeatures::default();
        let mut buffer_device_address_features =
            vk::PhysicalDeviceBufferDeviceAddressFeatures::default().buffer_device_address(true);
        let mut acceleration_structure_features =
            vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default()
                .acceleration_structure(true)
                .descriptor_binding_acceleration_structure_update_after_bind(false);
        let mut ray_tracing_features =
            vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true);
        let mut ray_query_features =
            vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true);

        let mut requested_features = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut descriptor_indexing_features)
            .push_next(&mut buffer_device_address_features)
            .push_next(&mut acceleration_structure_features)
            .push_next(&mut ray_tracing_features)
            .push_next(&mut ray_query_features);

        unsafe {
            instance.get_physical_device_features2(physical_device, &mut requested_features);
        }

        if buffer_device_address_features.buffer_device_address == vk::FALSE
            || acceleration_structure_features.acceleration_structure == vk::FALSE
            || ray_tracing_features.ray_tracing_pipeline == vk::FALSE
            || ray_query_features.ray_query == vk::FALSE
        {
            return Err(AppError::Message(
                "Selected GPU does not expose required Vulkan ray tracing features".to_string(),
            ));
        }

        let mut device_descriptor_indexing =
            vk::PhysicalDeviceDescriptorIndexingFeatures::default();
        let mut device_bda =
            vk::PhysicalDeviceBufferDeviceAddressFeatures::default().buffer_device_address(true);
        let mut device_as = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::default()
            .acceleration_structure(true);
        let mut device_rt =
            vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::default().ray_tracing_pipeline(true);
        let mut device_ray_query = vk::PhysicalDeviceRayQueryFeaturesKHR::default().ray_query(true);

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_extensions)
            .push_next(&mut device_descriptor_indexing)
            .push_next(&mut device_bda)
            .push_next(&mut device_as)
            .push_next(&mut device_rt)
            .push_next(&mut device_ray_query);

        let device = unsafe { instance.create_device(physical_device, &device_info, None)? };
        let graphics_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        let swapchain_loader = match mode {
            RendererMode::Windowed => Some(ash::khr::swapchain::Device::new(&instance, &device)),
            RendererMode::Headless => None,
        };
        let acceleration_loader = ash::khr::acceleration_structure::Device::new(&instance, &device);
        let ray_tracing_pipeline_loader =
            ash::khr::ray_tracing_pipeline::Device::new(&instance, &device);

        let mut rt_properties = vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();
        let mut as_properties = vk::PhysicalDeviceAccelerationStructurePropertiesKHR::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default()
            .push_next(&mut rt_properties)
            .push_next(&mut as_properties);
        unsafe { instance.get_physical_device_properties2(physical_device, &mut props2) };

        let device_name = unsafe { CStr::from_ptr(props2.properties.device_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let device_caps = RenderDeviceCaps {
            device_name,
            shader_group_handle_size: rt_properties.shader_group_handle_size,
            shader_group_base_alignment: rt_properties.shader_group_base_alignment,
            max_ray_recursion_depth: rt_properties.max_ray_recursion_depth,
        };

        let command_pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(queue_family_index)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?
        };
        let command_buffer = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?
        }[0];

        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device.clone(),
            physical_device,
            debug_settings: Default::default(),
            buffer_device_address: true,
            allocation_sizes: AllocationSizes::default(),
        })
        .map_err(|error| AppError::Message(error.to_string()))?;

        let image_available = if mode == RendererMode::Windowed {
            unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? }
        } else {
            vk::Semaphore::null()
        };
        let in_flight = unsafe {
            device.create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )?
        };

        let mut renderer = Self {
            _entry: entry,
            instance,
            debug_utils,
            debug_messenger,
            surface_loader,
            surface,
            physical_device,
            device,
            graphics_queue,
            swapchain_loader,
            swapchain: vk::SwapchainKHR::null(),
            swapchain_images: Vec::new(),
            swapchain_initialized: Vec::new(),
            swapchain_format: vk::SurfaceFormatKHR::default(),
            extent: vk::Extent2D {
                width: initial_size[0].max(1),
                height: initial_size[1].max(1),
            },
            allocator: Some(allocator),
            command_pool,
            command_buffer,
            image_available,
            render_finished: Vec::new(),
            in_flight,
            output_image: AllocatedImage::default(),
            capture_image: AllocatedImage::default(),
            readback_buffer: AllocatedBuffer::default(),
            uniform_buffer: AllocatedBuffer::default(),
            object_buffer: AllocatedBuffer::default(),
            voxel_buffer: AllocatedBuffer::default(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor_set_layout: vk::DescriptorSetLayout::null(),
            descriptor_set: vk::DescriptorSet::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            sbt: ShaderBindingTable::default(),
            acceleration_loader,
            ray_tracing_pipeline_loader,
            scene_acceleration: SceneAcceleration::default(),
            device_caps,
            mode,
        };

        match renderer.mode {
            RendererMode::Windowed => {
                renderer.recreate_swapchain(initial_size[0], initial_size[1])?;
            }
            RendererMode::Headless => {
                let storage_format = renderer.pick_headless_output_storage_format()?;
                renderer.output_image =
                    renderer.create_storage_image(renderer.extent, storage_format, "output image")?;
                renderer.capture_image = renderer.create_capture_image(renderer.extent)?;
                renderer.readback_buffer = renderer.create_readback_buffer(renderer.extent)?;
            }
        }
        renderer.create_descriptor_resources()?;
        renderer.uniform_buffer = renderer.create_buffer(
            "scene uniform",
            size_of::<crate::scene::SceneUniform>() as u64,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        renderer.object_buffer = renderer.create_buffer(
            "object buffer",
            (size_of::<RenderObjectData>() * MAX_OBJECTS) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        let object_template = RenderObjectData::from(VoxelProceduralObject::from(model));
        let initial_objects = vec![object_template; MAX_OBJECTS];
        write_bytes(
            &mut renderer.object_buffer,
            bytemuck::cast_slice(&initial_objects),
        )?;
        renderer.voxel_buffer = renderer.create_buffer(
            "voxel occupancy",
            (size_of::<u32>() * model.occupancy.len() * MAX_OBJECTS) as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
        )?;
        renderer.scene_acceleration = renderer.create_scene_acceleration(model)?;
        renderer.update_descriptor_set()?;
        renderer.populate_voxel_buffer(model)?;
        renderer.create_pipeline_and_sbt()?;

        info!(
            "Selected device {} (max recursion depth {}, shader group handle size {})",
            renderer.device_caps.device_name,
            renderer.device_caps.max_ray_recursion_depth,
            renderer.device_caps.shader_group_handle_size
        );

        Ok(renderer)
    }

    pub fn device_caps(&self) -> &RenderDeviceCaps {
        &self.device_caps
    }

    pub fn handle_resize(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        if self.mode != RendererMode::Windowed {
            return Ok(());
        }
        if width == 0 || height == 0 {
            return Ok(());
        }

        self.wait_idle()?;
        self.recreate_swapchain(width, height)?;
        self.update_descriptor_set()?;
        Ok(())
    }

    pub fn render(&mut self, scene: &ExtractedScene) -> Result<(), AppError> {
        if self.mode != RendererMode::Windowed {
            return Err(AppError::Message(
                "windowed render path was used for a headless renderer".to_string(),
            ));
        }
        if self.extent.width == 0 || self.extent.height == 0 {
            return Ok(());
        }

        self.upload_scene(scene)?;
        self.begin_frame_submission()?;

        let (image_index, suboptimal) = unsafe {
            match self
                .swapchain_loader
                .as_ref()
                .expect("windowed renderer should have a swapchain loader")
                .acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available,
                vk::Fence::null(),
            ) {
                Ok(pair) => pair,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate_swapchain(self.extent.width, self.extent.height)?;
                    self.update_descriptor_set()?;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        };

        unsafe {
            self.device
                .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
            self.device.begin_command_buffer(
                self.command_buffer,
                &vk::CommandBufferBeginInfo::default(),
            )?;
        }

        self.record_render_to_output_commands();
        self.record_present_commands(image_index as usize)?;

        unsafe {
            self.device.end_command_buffer(self.command_buffer)?;

            let wait_semaphores = [self.image_available];
            let wait_stages = [vk::PipelineStageFlags::TRANSFER];
            let signal_semaphores = [self.render_finished[image_index as usize]];
            let command_buffers = [self.command_buffer];
            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(&command_buffers)
                .signal_semaphores(&signal_semaphores);

            self.device
                .queue_submit(self.graphics_queue, &[submit_info], self.in_flight)?;

            let swapchains = [self.swapchain];
            let image_indices = [image_index];
            let present_wait = [self.render_finished[image_index as usize]];
            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&present_wait)
                .swapchains(&swapchains)
                .image_indices(&image_indices);

            match self
                .swapchain_loader
                .as_ref()
                .expect("windowed renderer should have a swapchain loader")
                .queue_present(self.graphics_queue, &present_info)
            {
                Ok(needs_resize) if needs_resize || suboptimal => {
                    self.recreate_swapchain(self.extent.width, self.extent.height)?;
                    self.update_descriptor_set()?;
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.recreate_swapchain(self.extent.width, self.extent.height)?;
                    self.update_descriptor_set()?;
                }
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
        }

        self.swapchain_initialized[image_index as usize] = true;
        self.output_image.layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;

        Ok(())
    }

    pub fn render_headless(&mut self, scene: &ExtractedScene) -> Result<(), AppError> {
        if self.mode != RendererMode::Headless {
            return Err(AppError::Message(
                "headless render path was used for a windowed renderer".to_string(),
            ));
        }
        if self.extent.width == 0 || self.extent.height == 0 {
            return Ok(());
        }

        self.upload_scene(scene)?;
        self.begin_frame_submission()?;

        unsafe {
            self.device
                .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
            self.device.begin_command_buffer(
                self.command_buffer,
                &vk::CommandBufferBeginInfo::default(),
            )?;
        }

        self.record_render_to_output_commands();

        unsafe {
            self.device.end_command_buffer(self.command_buffer)?;
            let command_buffers = [self.command_buffer];
            let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);
            self.device
                .queue_submit(self.graphics_queue, &[submit_info], self.in_flight)?;
        }

        self.output_image.layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        Ok(())
    }

    pub fn save_headless_png(&mut self, path: &Path) -> Result<(), AppError> {
        if self.mode != RendererMode::Headless {
            return Err(AppError::Message(
                "PNG capture is only available for a headless renderer".to_string(),
            ));
        }

        self.wait_for_in_flight()?;
        self.immediate_submit(|renderer, command_buffer| {
            renderer.record_capture_commands(command_buffer);
        })?;
        self.capture_image.layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;

        let expected_len = rgba_image_byte_len(self.extent)?;
        let bytes = read_bytes(&self.readback_buffer, expected_len)?;
        write_png(path, self.extent, &bytes)
    }

    pub fn wait_idle(&mut self) -> Result<(), AppError> {
        unsafe { self.device.device_wait_idle()? };
        Ok(())
    }

    fn wait_for_in_flight(&self) -> Result<(), AppError> {
        unsafe {
            self.device.wait_for_fences(&[self.in_flight], true, u64::MAX)?;
        }
        Ok(())
    }

    fn begin_frame_submission(&self) -> Result<(), AppError> {
        self.wait_for_in_flight()?;
        unsafe {
            self.device.reset_fences(&[self.in_flight])?;
        }
        Ok(())
    }

    fn recreate_swapchain(&mut self, width: u32, height: u32) -> Result<(), AppError> {
        let surface_loader = self
            .surface_loader
            .as_ref()
            .expect("windowed renderer should have a surface loader");
        let capabilities = unsafe {
            surface_loader.get_physical_device_surface_capabilities(self.physical_device, self.surface)?
        };
        let formats = unsafe {
            surface_loader.get_physical_device_surface_formats(self.physical_device, self.surface)?
        };
        let present_modes = unsafe {
            surface_loader.get_physical_device_surface_present_modes(self.physical_device, self.surface)?
        };

        let chosen_format = formats
            .iter()
            .copied()
            .find(|format| {
                (format.format == vk::Format::B8G8R8A8_UNORM
                    || format.format == vk::Format::B8G8R8A8_SRGB
                    || format.format == vk::Format::R8G8B8A8_UNORM)
                    && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .unwrap_or_else(|| formats[0]);

        let present_mode = present_modes
            .into_iter()
            .find(|mode| *mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        self.extent = vk::Extent2D {
            width: width.clamp(
                capabilities.min_image_extent.width.max(1),
                capabilities.max_image_extent.width.max(1),
            ),
            height: height.clamp(
                capabilities.min_image_extent.height.max(1),
                capabilities.max_image_extent.height.max(1),
            ),
        };

        let desired_image_count = (capabilities.min_image_count + 1).min(
            capabilities
                .max_image_count
                .max(capabilities.min_image_count + 1),
        );

        let usage = vk::ImageUsageFlags::TRANSFER_DST;
        if !capabilities.supported_usage_flags.contains(usage) {
            return Err(AppError::Message(
                "Surface does not support transfer-destination swapchain images".to_string(),
            ));
        }

        let create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.surface)
            .min_image_count(desired_image_count)
            .image_format(chosen_format.format)
            .image_color_space(chosen_format.color_space)
            .image_extent(self.extent)
            .image_array_layers(1)
            .image_usage(usage)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(capabilities.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(self.swapchain);

        let new_swapchain = unsafe {
            self.swapchain_loader
                .as_ref()
                .expect("windowed renderer should have a swapchain loader")
                .create_swapchain(&create_info, None)?
        };
        let new_images = unsafe {
            self.swapchain_loader
                .as_ref()
                .expect("windowed renderer should have a swapchain loader")
                .get_swapchain_images(new_swapchain)?
        };
        let output_image = self.create_storage_image(
            self.extent,
            self.pick_windowed_output_storage_format(chosen_format.format)?,
            "output image",
        )?;

        if self.swapchain != vk::SwapchainKHR::null() {
            unsafe {
                self.swapchain_loader
                    .as_ref()
                    .expect("windowed renderer should have a swapchain loader")
                    .destroy_swapchain(self.swapchain, None)
            };
        }
        destroy_image_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.output_image,
        )?;

        self.swapchain = new_swapchain;
        self.swapchain_images = new_images;
        self.swapchain_initialized = vec![false; self.swapchain_images.len()];
        self.swapchain_format = chosen_format;
        self.output_image = output_image;
        self.recreate_render_finished_semaphores(self.swapchain_images.len())?;

        Ok(())
    }

    fn create_descriptor_resources(&mut self) -> Result<(), AppError> {
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR),
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR),
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR),
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(
                    vk::ShaderStageFlags::INTERSECTION_KHR
                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR
                        | vk::ShaderStageFlags::COMPUTE,
                ),
            vk::DescriptorSetLayoutBinding::default()
                .binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(
                    vk::ShaderStageFlags::INTERSECTION_KHR
                        | vk::ShaderStageFlags::CLOSEST_HIT_KHR
                        | vk::ShaderStageFlags::COMPUTE,
                ),
        ];

        self.descriptor_set_layout = unsafe {
            self.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )?
        };

        let pool_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_IMAGE,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: 1,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: 2,
            },
        ];

        self.descriptor_pool = unsafe {
            self.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&pool_sizes)
                    .max_sets(1),
                None,
            )?
        };
        self.descriptor_set = unsafe {
            self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.descriptor_pool)
                    .set_layouts(&[self.descriptor_set_layout]),
            )?
        }[0];

        self.pipeline_layout = unsafe {
            self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[self.descriptor_set_layout]),
                None,
            )?
        };

        Ok(())
    }

    fn create_scene_acceleration(
        &mut self,
        model: &VoxelModel,
    ) -> Result<SceneAcceleration, AppError> {
        let object = VoxelProceduralObject::from(model);
        let instance_positions = terrain_grid_positions(object.extent());
        let aabb = vk::AabbPositionsKHR {
            min_x: object.bounds_min.x,
            min_y: object.bounds_min.y,
            min_z: object.bounds_min.z,
            max_x: object.bounds_max.x,
            max_y: object.bounds_max.y,
            max_z: object.bounds_max.z,
        };

        let mut aabb_buffer = self.create_buffer(
            "procedural aabb",
            size_of::<vk::AabbPositionsKHR>() as u64,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::CpuToGpu,
        )?;
        write_bytes(&mut aabb_buffer, unsafe {
            std::slice::from_raw_parts(
                (&aabb as *const vk::AabbPositionsKHR).cast::<u8>(),
                size_of::<vk::AabbPositionsKHR>(),
            )
        })?;

        let aabb_address = self.buffer_device_address(aabb_buffer.buffer);
        let geometry_aabb_data = vk::AccelerationStructureGeometryAabbsDataKHR::default()
            .data(vk::DeviceOrHostAddressConstKHR {
                device_address: aabb_address,
            })
            .stride(size_of::<vk::AabbPositionsKHR>() as u64);

        let geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::AABBS)
            .flags(vk::GeometryFlagsKHR::OPAQUE)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                aabbs: geometry_aabb_data,
            });

        let blas_geometries = [geometry];
        let mut blas_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .geometries(&blas_geometries);
        let primitive_counts = [1u32];
        let mut blas_sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            self.acceleration_loader
                .get_acceleration_structure_build_sizes(
                    vk::AccelerationStructureBuildTypeKHR::DEVICE,
                    &blas_build_info,
                    &primitive_counts,
                    &mut blas_sizes,
                );
        }

        let blas = self.create_acceleration_structure_resource(
            "blas",
            vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL,
            blas_sizes.acceleration_structure_size,
        )?;
        let mut blas_scratch = self.create_buffer(
            "blas scratch",
            blas_sizes.build_scratch_size,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::GpuOnly,
        )?;

        blas_build_info = blas_build_info
            .dst_acceleration_structure(blas.handle)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: self.buffer_device_address(blas_scratch.buffer),
            });

        let blas_range = [vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: 1,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        }];

        self.immediate_submit(|renderer, command_buffer| unsafe {
            renderer
                .acceleration_loader
                .cmd_build_acceleration_structures(
                    command_buffer,
                    &[blas_build_info],
                    &[&blas_range],
                );
        })?;
        destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut blas_scratch,
        )?;

        let instances = instance_positions
            .iter()
            .enumerate()
            .map(|(index, position)| vk::AccelerationStructureInstanceKHR {
                transform: vk::TransformMatrixKHR {
                    matrix: [
                        1.0, 0.0, 0.0, position.x, 0.0, 1.0, 0.0, position.y, 0.0, 0.0, 1.0,
                        position.z,
                    ],
                },
                instance_custom_index_and_mask: vk::Packed24_8::new(index as u32, 0xFF),
                instance_shader_binding_table_record_offset_and_flags: vk::Packed24_8::new(
                    0,
                    vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
                ),
                acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                    device_handle: blas.device_address,
                },
            })
            .collect::<Vec<_>>();

        let mut instance_buffer = self.create_buffer(
            "tlas instances",
            (size_of::<vk::AccelerationStructureInstanceKHR>() * instances.len()) as u64,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::CpuToGpu,
        )?;
        write_bytes(&mut instance_buffer, unsafe {
            std::slice::from_raw_parts(
                instances.as_ptr().cast::<u8>(),
                size_of::<vk::AccelerationStructureInstanceKHR>() * instances.len(),
            )
        })?;

        let instances_address = self.buffer_device_address(instance_buffer.buffer);
        let geometry_instances = vk::AccelerationStructureGeometryInstancesDataKHR::default()
            .array_of_pointers(false)
            .data(vk::DeviceOrHostAddressConstKHR {
                device_address: instances_address,
            });
        let tlas_geometry = vk::AccelerationStructureGeometryKHR::default()
            .geometry_type(vk::GeometryTypeKHR::INSTANCES)
            .geometry(vk::AccelerationStructureGeometryDataKHR {
                instances: geometry_instances,
            });

        let tlas_geometries = [tlas_geometry];
        let mut tlas_build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
            .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .geometries(&tlas_geometries);
        let tlas_counts = [instances.len() as u32];
        let mut tlas_sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
        unsafe {
            self.acceleration_loader
                .get_acceleration_structure_build_sizes(
                    vk::AccelerationStructureBuildTypeKHR::DEVICE,
                    &tlas_build_info,
                    &tlas_counts,
                    &mut tlas_sizes,
                );
        }

        let tlas = self.create_acceleration_structure_resource(
            "tlas",
            vk::AccelerationStructureTypeKHR::TOP_LEVEL,
            tlas_sizes.acceleration_structure_size,
        )?;
        let mut tlas_scratch = self.create_buffer(
            "tlas scratch",
            tlas_sizes.build_scratch_size,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::GpuOnly,
        )?;

        tlas_build_info = tlas_build_info
            .dst_acceleration_structure(tlas.handle)
            .scratch_data(vk::DeviceOrHostAddressKHR {
                device_address: self.buffer_device_address(tlas_scratch.buffer),
            });

        let tlas_range = [vk::AccelerationStructureBuildRangeInfoKHR {
            primitive_count: instances.len() as u32,
            primitive_offset: 0,
            first_vertex: 0,
            transform_offset: 0,
        }];

        self.immediate_submit(|renderer, command_buffer| unsafe {
            renderer
                .acceleration_loader
                .cmd_build_acceleration_structures(
                    command_buffer,
                    &[tlas_build_info],
                    &[&tlas_range],
                );
        })?;
        destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut tlas_scratch,
        )?;

        Ok(SceneAcceleration {
            aabb_buffer,
            instance_buffer,
            blas,
            tlas,
        })
    }

    fn create_pipeline_and_sbt(&mut self) -> Result<(), AppError> {
        let artifact_paths = compiled_shader_artifacts();
        let stage_defs = [
            (
                artifact_paths[0].clone(),
                c"raygen_main",
                vk::ShaderStageFlags::RAYGEN_KHR,
            ),
            (
                artifact_paths[1].clone(),
                c"miss_main",
                vk::ShaderStageFlags::MISS_KHR,
            ),
            (
                artifact_paths[2].clone(),
                c"closest_hit_main",
                vk::ShaderStageFlags::CLOSEST_HIT_KHR,
            ),
            (
                artifact_paths[3].clone(),
                c"intersection_main",
                vk::ShaderStageFlags::INTERSECTION_KHR,
            ),
        ];

        let mut shader_modules = Vec::new();
        let mut stages = Vec::new();
        for (path, entry_name, stage) in stage_defs {
            let bytes = std::fs::read(&path)?;
            let code = read_spv(&mut Cursor::new(bytes))
                .map_err(|error| AppError::Message(error.to_string()))?;
            let module = unsafe {
                self.device.create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&code),
                    None,
                )?
            };
            shader_modules.push(module);
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .module(module)
                    .name(entry_name)
                    .stage(stage),
            );
        }

        let groups = [
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(0)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(1)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR),
            vk::RayTracingShaderGroupCreateInfoKHR::default()
                .ty(vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(2)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(3),
        ];

        let pipeline_info = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stages)
            .groups(&groups)
            .max_pipeline_ray_recursion_depth(1)
            .layout(self.pipeline_layout);

        self.pipeline = unsafe {
            self.ray_tracing_pipeline_loader
                .create_ray_tracing_pipelines(
                    vk::DeferredOperationKHR::null(),
                    vk::PipelineCache::null(),
                    &[pipeline_info],
                    None,
                )
                .map_err(|(_, error)| error)?
        }[0];

        for module in shader_modules {
            unsafe { self.device.destroy_shader_module(module, None) };
        }

        self.sbt = self.create_shader_binding_table(groups.len() as u32)?;

        Ok(())
    }

    fn populate_voxel_buffer(&mut self, model: &VoxelModel) -> Result<(), AppError> {
        if model.occupancy.iter().any(|value| *value != 0) {
            let mut repeated = Vec::with_capacity(model.occupancy.len() * MAX_OBJECTS);
            for _ in 0..MAX_OBJECTS {
                repeated.extend_from_slice(&model.occupancy);
            }
            write_bytes(&mut self.voxel_buffer, bytemuck::cast_slice(&repeated))?;
            return Ok(());
        }

        self.generate_terrain_voxels(model)
    }

    fn generate_terrain_voxels(&mut self, model: &VoxelModel) -> Result<(), AppError> {
        let shader_path = compiled_shader_artifact("terrain_gen.spv");
        let bytes = std::fs::read(&shader_path)?;
        let code = read_spv(&mut Cursor::new(bytes))
            .map_err(|error| AppError::Message(error.to_string()))?;
        let shader_module = unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)?
        };

        let stage = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module)
            .name(c"terrain_gen_main")
            .stage(vk::ShaderStageFlags::COMPUTE);
        let pipeline_info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(self.pipeline_layout);

        let pipeline = unsafe {
            self.device
                .create_compute_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
                .map_err(|(_, error)| error)?
        }[0];

        let group_count_x = model.dimensions.x.div_ceil(8);
        let group_count_y = model.dimensions.y.div_ceil(4);
        let total_depth = model.dimensions.z * MAX_OBJECTS as u32;
        let group_count_z = total_depth.div_ceil(8);

        self.immediate_submit(|renderer, command_buffer| unsafe {
            renderer.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                pipeline,
            );
            renderer.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::COMPUTE,
                renderer.pipeline_layout,
                0,
                &[renderer.descriptor_set],
                &[],
            );
            renderer.device.cmd_dispatch(
                command_buffer,
                group_count_x,
                group_count_y,
                group_count_z,
            );
            renderer.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)],
                &[],
                &[],
            );
        })?;

        unsafe {
            self.device.destroy_pipeline(pipeline, None);
            self.device.destroy_shader_module(shader_module, None);
        }

        Ok(())
    }

    fn create_shader_binding_table(
        &mut self,
        group_count: u32,
    ) -> Result<ShaderBindingTable, AppError> {
        let handle_size = self.device_caps.shader_group_handle_size as usize;
        let handle_alignment = self.device_caps.shader_group_base_alignment as usize;
        let aligned_handle_size = align_up(handle_size, align_of::<u64>());
        let region_stride = align_up(aligned_handle_size, handle_alignment);

        let handles = unsafe {
            self.ray_tracing_pipeline_loader
                .get_ray_tracing_shader_group_handles(
                    self.pipeline,
                    0,
                    group_count,
                    handle_size * group_count as usize,
                )?
        };

        let mut buffer = self.create_buffer(
            "shader binding table",
            (region_stride * group_count as usize) as u64,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
            MemoryLocation::CpuToGpu,
        )?;
        let mapped = buffer
            .allocation
            .as_mut()
            .and_then(Allocation::mapped_slice_mut)
            .ok_or_else(|| {
                AppError::Message("shader binding table buffer is not mapped".to_string())
            })?;
        for group in 0..group_count as usize {
            let src_offset = group * handle_size;
            let dst_offset = group * region_stride;
            mapped[dst_offset..dst_offset + handle_size]
                .copy_from_slice(&handles[src_offset..src_offset + handle_size]);
        }

        let base_address = self.buffer_device_address(buffer.buffer);
        let raygen_region = vk::StridedDeviceAddressRegionKHR::default()
            .device_address(base_address)
            .stride(region_stride as u64)
            .size(region_stride as u64);
        let miss_region = vk::StridedDeviceAddressRegionKHR::default()
            .device_address(base_address + region_stride as u64)
            .stride(region_stride as u64)
            .size(region_stride as u64);
        let hit_region = vk::StridedDeviceAddressRegionKHR::default()
            .device_address(base_address + (region_stride * 2) as u64)
            .stride(region_stride as u64)
            .size(region_stride as u64);

        Ok(ShaderBindingTable {
            buffer,
            raygen_region,
            miss_region,
            hit_region,
        })
    }

    fn update_descriptor_set(&mut self) -> Result<(), AppError> {
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::GENERAL)
            .image_view(self.output_image.view)];
        let uniform_info = [vk::DescriptorBufferInfo::default()
            .buffer(self.uniform_buffer.buffer)
            .offset(0)
            .range(self.uniform_buffer.size)];
        let object_info = [vk::DescriptorBufferInfo::default()
            .buffer(self.object_buffer.buffer)
            .offset(0)
            .range(self.object_buffer.size)];
        let voxel_info = [vk::DescriptorBufferInfo::default()
            .buffer(self.voxel_buffer.buffer)
            .offset(0)
            .range(self.voxel_buffer.size)];
        let acceleration_structures = [self.scene_acceleration.tlas.handle];
        let mut acceleration_write = vk::WriteDescriptorSetAccelerationStructureKHR::default()
            .acceleration_structures(&acceleration_structures);

        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .image_info(&image_info),
            vk::WriteDescriptorSet::default()
                .push_next(&mut acceleration_write)
                .dst_set(self.descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                .descriptor_count(1),
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&uniform_info),
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&object_info),
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_set)
                .dst_binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&voxel_info),
        ];

        unsafe { self.device.update_descriptor_sets(&writes, &[]) };
        Ok(())
    }

    fn upload_scene(&mut self, scene: &ExtractedScene) -> Result<(), AppError> {
        if scene.objects.is_empty() {
            return Err(AppError::Message(
                "extracted scene has no render objects".to_string(),
            ));
        }
        if scene.objects.len() > MAX_OBJECTS {
            return Err(AppError::Message(format!(
                "scene contains {} objects, but only {} are currently supported",
                scene.objects.len(),
                MAX_OBJECTS
            )));
        }

        write_bytes(&mut self.uniform_buffer, bytemuck::bytes_of(&scene.camera))?;
        write_bytes(
            &mut self.object_buffer,
            bytemuck::cast_slice(&scene.objects[..scene.objects.len()]),
        )?;

        Ok(())
    }

    fn record_render_to_output_commands(&self) {
        transition_image(
            &self.device,
            self.command_buffer,
            self.output_image.image,
            self.output_image.layout,
            vk::ImageLayout::GENERAL,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::AccessFlags::TRANSFER_READ,
            vk::AccessFlags::SHADER_WRITE,
        );

        unsafe {
            self.device.cmd_bind_pipeline(
                self.command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                self.pipeline,
            );
            self.device.cmd_bind_descriptor_sets(
                self.command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                self.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );
            self.ray_tracing_pipeline_loader.cmd_trace_rays(
                self.command_buffer,
                &self.sbt.raygen_region,
                &self.sbt.miss_region,
                &self.sbt.hit_region,
                &vk::StridedDeviceAddressRegionKHR::default(),
                self.extent.width,
                self.extent.height,
                1,
            );
        }

        transition_image(
            &self.device,
            self.command_buffer,
            self.output_image.image,
            vk::ImageLayout::GENERAL,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR,
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::SHADER_WRITE,
            vk::AccessFlags::TRANSFER_READ,
        );
    }

    fn record_present_commands(&mut self, image_index: usize) -> Result<(), AppError> {
        let swapchain_old_layout = if self.swapchain_initialized[image_index] {
            vk::ImageLayout::PRESENT_SRC_KHR
        } else {
            vk::ImageLayout::UNDEFINED
        };

        transition_image(
            &self.device,
            self.command_buffer,
            self.swapchain_images[image_index],
            swapchain_old_layout,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::empty(),
            vk::AccessFlags::TRANSFER_WRITE,
        );

        if self.output_image.format == self.swapchain_format.format {
            unsafe {
                self.device.cmd_copy_image(
                    self.command_buffer,
                    self.output_image.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    self.swapchain_images[image_index],
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageCopy::default()
                        .src_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .dst_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .extent(self.extent.into())],
                );
            }
        } else {
            unsafe {
                self.device.cmd_blit_image(
                    self.command_buffer,
                    self.output_image.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    self.swapchain_images[image_index],
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageBlit::default()
                        .src_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .src_offsets([
                            vk::Offset3D { x: 0, y: 0, z: 0 },
                            vk::Offset3D {
                                x: self.extent.width as i32,
                                y: self.extent.height as i32,
                                z: 1,
                            },
                        ])
                        .dst_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .dst_offsets([
                            vk::Offset3D { x: 0, y: 0, z: 0 },
                            vk::Offset3D {
                                x: self.extent.width as i32,
                                y: self.extent.height as i32,
                                z: 1,
                            },
                        ])],
                    vk::Filter::NEAREST,
                );
            }
        }

        transition_image(
            &self.device,
            self.command_buffer,
            self.swapchain_images[image_index],
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::PRESENT_SRC_KHR,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::empty(),
        );

        Ok(())
    }

    fn create_storage_image(
        &mut self,
        extent: vk::Extent2D,
        format: vk::Format,
        name: &str,
    ) -> Result<AllocatedImage, AppError> {
        let image = unsafe {
            self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?
        };
        let requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let allocation = self
            .allocator
            .as_mut()
            .expect("renderer allocator should exist")
            .allocate(&AllocationCreateDesc {
                name,
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;
        unsafe {
            self.device
                .bind_image_memory(image, allocation.memory(), allocation.offset())?;
        }

        let view = unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )?
        };

        Ok(AllocatedImage {
            image,
            view,
            allocation: Some(allocation),
            format,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    fn pick_windowed_output_storage_format(
        &self,
        preferred_format: vk::Format,
    ) -> Result<vk::Format, AppError> {
        let swapchain_props = unsafe {
            self.instance
                .get_physical_device_format_properties(self.physical_device, preferred_format)
        };
        let swapchain_can_blit_dst = swapchain_props
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::BLIT_DST);

        if preferred_format == vk::Format::R32G32B32A32_SFLOAT {
            let props = unsafe {
                self.instance
                    .get_physical_device_format_properties(self.physical_device, preferred_format)
            };
            if props
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::STORAGE_IMAGE)
            {
                return Ok(preferred_format);
            }
        }

        let float_output = HEADLESS_OUTPUT_FORMAT;
        let props = unsafe {
            self.instance
                .get_physical_device_format_properties(self.physical_device, float_output)
        };
        if swapchain_can_blit_dst
            && props
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::STORAGE_IMAGE | vk::FormatFeatureFlags::BLIT_SRC)
        {
            return Ok(float_output);
        }

        Err(AppError::Message(format!(
            "could not find an R32G32B32A32_SFLOAT storage output format compatible with swapchain format {preferred_format:?}"
        )))
    }

    fn pick_headless_output_storage_format(&self) -> Result<vk::Format, AppError> {
        let props = unsafe {
            self.instance
                .get_physical_device_format_properties(self.physical_device, HEADLESS_OUTPUT_FORMAT)
        };
        if props.optimal_tiling_features.contains(
            vk::FormatFeatureFlags::STORAGE_IMAGE | vk::FormatFeatureFlags::BLIT_SRC,
        ) {
            return Ok(HEADLESS_OUTPUT_FORMAT);
        }

        Err(AppError::Message(
            "could not find an R32G32B32A32_SFLOAT storage output format for headless capture"
                .to_string(),
        ))
    }

    fn create_capture_image(&mut self, extent: vk::Extent2D) -> Result<AllocatedImage, AppError> {
        let props = unsafe {
            self.instance
                .get_physical_device_format_properties(self.physical_device, HEADLESS_CAPTURE_FORMAT)
        };
        if !props
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::BLIT_DST)
        {
            return Err(AppError::Message(
                "R8G8B8A8_UNORM capture images do not support blit-destination usage".to_string(),
            ));
        }

        let image = unsafe {
            self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(HEADLESS_CAPTURE_FORMAT)
                    .extent(vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?
        };
        let requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let allocation = self
            .allocator
            .as_mut()
            .expect("renderer allocator should exist")
            .allocate(&AllocationCreateDesc {
                name: "capture image",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;
        unsafe {
            self.device
                .bind_image_memory(image, allocation.memory(), allocation.offset())?;
        }

        Ok(AllocatedImage {
            image,
            view: vk::ImageView::null(),
            allocation: Some(allocation),
            format: HEADLESS_CAPTURE_FORMAT,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    fn create_readback_buffer(&mut self, extent: vk::Extent2D) -> Result<AllocatedBuffer, AppError> {
        self.create_buffer(
            "capture readback",
            rgba_image_byte_len(extent)? as u64,
            vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
        )
    }

    fn record_capture_commands(&self, command_buffer: vk::CommandBuffer) {
        transition_image(
            &self.device,
            command_buffer,
            self.capture_image.image,
            self.capture_image.layout,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ,
            vk::AccessFlags::TRANSFER_WRITE,
        );

        unsafe {
            self.device.cmd_blit_image(
                command_buffer,
                self.output_image.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.capture_image.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageBlit::default()
                    .src_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .src_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: self.extent.width as i32,
                            y: self.extent.height as i32,
                            z: 1,
                        },
                    ])
                    .dst_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .dst_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: self.extent.width as i32,
                            y: self.extent.height as i32,
                            z: 1,
                        },
                    ])],
                vk::Filter::NEAREST,
            );
        }

        transition_image(
            &self.device,
            command_buffer,
            self.capture_image.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::TRANSFER_READ,
        );

        unsafe {
            self.device.cmd_copy_image_to_buffer(
                command_buffer,
                self.capture_image.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.readback_buffer.buffer,
                &[vk::BufferImageCopy::default()
                    .buffer_offset(0)
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(self.extent.into())],
            );
            self.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[vk::BufferMemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::HOST_READ)
                    .buffer(self.readback_buffer.buffer)
                    .offset(0)
                    .size(self.readback_buffer.size)],
                &[],
            );
        }
    }

    fn recreate_render_finished_semaphores(&mut self, count: usize) -> Result<(), AppError> {
        for semaphore in self.render_finished.drain(..) {
            unsafe { self.device.destroy_semaphore(semaphore, None) };
        }
        self.render_finished = (0..count)
            .map(|_| unsafe {
                self.device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    fn create_acceleration_structure_resource(
        &mut self,
        name: &str,
        ty: vk::AccelerationStructureTypeKHR,
        size: vk::DeviceSize,
    ) -> Result<AccelerationStructureResource, AppError> {
        let buffer = self.create_buffer(
            name,
            size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            MemoryLocation::GpuOnly,
        )?;
        let handle = unsafe {
            self.acceleration_loader.create_acceleration_structure(
                &vk::AccelerationStructureCreateInfoKHR::default()
                    .buffer(buffer.buffer)
                    .offset(0)
                    .size(size)
                    .ty(ty),
                None,
            )?
        };
        let device_address = unsafe {
            self.acceleration_loader
                .get_acceleration_structure_device_address(
                    &vk::AccelerationStructureDeviceAddressInfoKHR::default()
                        .acceleration_structure(handle),
                )
        };

        Ok(AccelerationStructureResource {
            handle,
            buffer,
            device_address,
        })
    }

    fn create_buffer(
        &mut self,
        name: &str,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Result<AllocatedBuffer, AppError> {
        let buffer = unsafe {
            self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size.max(1))
                    .usage(usage | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?
        };
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let allocation = self
            .allocator
            .as_mut()
            .expect("renderer allocator should exist")
            .allocate(&AllocationCreateDesc {
                name,
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|error| AppError::Message(error.to_string()))?;
        unsafe {
            self.device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;
        }

        Ok(AllocatedBuffer {
            buffer,
            allocation: Some(allocation),
            size,
        })
    }

    fn buffer_device_address(&self, buffer: vk::Buffer) -> vk::DeviceAddress {
        unsafe {
            self.device
                .get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(buffer))
        }
    }

    fn immediate_submit<F>(&mut self, record: F) -> Result<(), AppError>
    where
        F: FnOnce(&Self, vk::CommandBuffer),
    {
        unsafe {
            self.device
                .reset_command_pool(self.command_pool, vk::CommandPoolResetFlags::empty())?;
            self.device.begin_command_buffer(
                self.command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }

        record(self, self.command_buffer);

        unsafe {
            self.device.end_command_buffer(self.command_buffer)?;
            let command_buffers = [self.command_buffer];
            let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);
            self.device
                .queue_submit(self.graphics_queue, &[submit_info], vk::Fence::null())?;
            self.device.queue_wait_idle(self.graphics_queue)?;
        }

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.wait_idle();

        unsafe {
            if self.pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(self.pipeline, None);
            }
            if self.pipeline_layout != vk::PipelineLayout::null() {
                self.device
                    .destroy_pipeline_layout(self.pipeline_layout, None);
            }
        }
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.sbt.buffer,
        );
        if self.descriptor_pool != vk::DescriptorPool::null() {
            unsafe {
                self.device
                    .destroy_descriptor_pool(self.descriptor_pool, None)
            };
        }
        if self.descriptor_set_layout != vk::DescriptorSetLayout::null() {
            unsafe {
                self.device
                    .destroy_descriptor_set_layout(self.descriptor_set_layout, None)
            };
        }

        unsafe {
            if self.scene_acceleration.blas.handle != vk::AccelerationStructureKHR::null() {
                self.acceleration_loader
                    .destroy_acceleration_structure(self.scene_acceleration.blas.handle, None);
            }
            if self.scene_acceleration.tlas.handle != vk::AccelerationStructureKHR::null() {
                self.acceleration_loader
                    .destroy_acceleration_structure(self.scene_acceleration.tlas.handle, None);
            }
        }
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.scene_acceleration.blas.buffer,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.scene_acceleration.tlas.buffer,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.scene_acceleration.aabb_buffer,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.scene_acceleration.instance_buffer,
        );

        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.uniform_buffer,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.object_buffer,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.voxel_buffer,
        );
        let _ = destroy_image_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.output_image,
        );
        let _ = destroy_image_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.capture_image,
        );
        let _ = destroy_buffer_with(
            &self.device,
            self.allocator
                .as_mut()
                .expect("renderer allocator should exist"),
            &mut self.readback_buffer,
        );
        self.allocator.take();

        if self.swapchain != vk::SwapchainKHR::null() {
            if let Some(swapchain_loader) = &self.swapchain_loader {
                unsafe { swapchain_loader.destroy_swapchain(self.swapchain, None) };
            }
        }

        unsafe {
            if self.image_available != vk::Semaphore::null() {
                self.device.destroy_semaphore(self.image_available, None);
            }
            for semaphore in self.render_finished.drain(..) {
                self.device.destroy_semaphore(semaphore, None);
            }
            if self.in_flight != vk::Fence::null() {
                self.device.destroy_fence(self.in_flight, None);
            }
            if self.command_pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.command_pool, None);
            }
            self.device.destroy_device(None);
        }

        if let Some(surface_loader) = &self.surface_loader {
            unsafe {
                surface_loader.destroy_surface(self.surface, None);
            }
        }
        if let (Some(loader), Some(messenger)) = (&self.debug_utils, self.debug_messenger) {
            unsafe { loader.destroy_debug_utils_messenger(messenger, None) };
        }
        unsafe {
            self.instance.destroy_instance(None);
        }
    }
}

#[derive(Default)]
struct SceneAcceleration {
    aabb_buffer: AllocatedBuffer,
    instance_buffer: AllocatedBuffer,
    blas: AccelerationStructureResource,
    tlas: AccelerationStructureResource,
}

#[derive(Default)]
struct AccelerationStructureResource {
    handle: vk::AccelerationStructureKHR,
    buffer: AllocatedBuffer,
    device_address: vk::DeviceAddress,
}

#[derive(Default)]
struct ShaderBindingTable {
    buffer: AllocatedBuffer,
    raygen_region: vk::StridedDeviceAddressRegionKHR,
    miss_region: vk::StridedDeviceAddressRegionKHR,
    hit_region: vk::StridedDeviceAddressRegionKHR,
}

#[derive(Default)]
struct AllocatedBuffer {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
    size: vk::DeviceSize,
}

#[derive(Default)]
struct AllocatedImage {
    image: vk::Image,
    view: vk::ImageView,
    allocation: Option<Allocation>,
    format: vk::Format,
    layout: vk::ImageLayout,
}

fn pick_physical_device(
    instance: &Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    required_extensions: &[&CStr],
) -> Result<vk::PhysicalDevice, AppError> {
    let physical_devices = unsafe { instance.enumerate_physical_devices()? };

    let mut candidates = physical_devices
        .into_iter()
        .filter_map(|device| {
            score_physical_device(
                instance,
                Some((surface_loader, surface)),
                device,
                required_extensions,
            )
            .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    candidates.sort_by_key(|(score, _)| *score);
    candidates.pop().map(|(_, device)| device).ok_or_else(|| {
        AppError::Message("No Vulkan physical device with ray tracing support found".to_string())
    })
}

fn pick_headless_physical_device(instance: &Instance) -> Result<vk::PhysicalDevice, AppError> {
    let physical_devices = unsafe { instance.enumerate_physical_devices()? };

    let mut candidates = physical_devices
        .into_iter()
        .filter_map(|device| {
            score_physical_device(instance, None, device, HEADLESS_DEVICE_EXTENSIONS).transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    candidates.sort_by_key(|(score, _)| *score);
    candidates.pop().map(|(_, device)| device).ok_or_else(|| {
        AppError::Message("No Vulkan physical device with ray tracing support found".to_string())
    })
}

fn score_physical_device(
    instance: &Instance,
    present_surface: Option<(&ash::khr::surface::Instance, vk::SurfaceKHR)>,
    physical_device: vk::PhysicalDevice,
    required_extensions: &[&CStr],
) -> Result<Option<(u32, vk::PhysicalDevice)>, AppError> {
    let queue_family = match present_surface {
        Some((surface_loader, surface)) => {
            match pick_queue_family(instance, surface_loader, physical_device, surface) {
                Ok(index) => index,
                Err(_) => return Ok(None),
            }
        }
        None => match pick_graphics_queue_family(instance, physical_device) {
            Ok(index) => index,
            Err(_) => return Ok(None),
        },
    };

    let extension_props =
        unsafe { instance.enumerate_device_extension_properties(physical_device)? };
    for required in required_extensions {
        let found = extension_props.iter().any(|extension| unsafe {
            CStr::from_ptr(extension.extension_name.as_ptr()) == *required
        });
        if !found {
            return Ok(None);
        }
    }

    let properties = unsafe { instance.get_physical_device_properties(physical_device) };
    let score = match properties.device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => 3,
        vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
        _ => 1,
    } + queue_family;

    Ok(Some((score, physical_device)))
}

fn pick_graphics_queue_family(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<u32, AppError> {
    let queue_families =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    for (index, family) in queue_families.into_iter().enumerate() {
        if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            return Ok(index as u32);
        }
    }

    Err(AppError::Message(
        "No graphics queue family was found".to_string(),
    ))
}

fn pick_queue_family(
    instance: &Instance,
    surface_loader: &ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> Result<u32, AppError> {
    let queue_families =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    for (index, family) in queue_families.into_iter().enumerate() {
        let supports_present = unsafe {
            surface_loader.get_physical_device_surface_support(
                physical_device,
                index as u32,
                surface,
            )?
        };
        if supports_present && family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            return Ok(index as u32);
        }
    }

    Err(AppError::Message(
        "No graphics queue family that can present to the surface was found".to_string(),
    ))
}

fn write_bytes(buffer: &mut AllocatedBuffer, data: &[u8]) -> Result<(), AppError> {
    let allocation = buffer
        .allocation
        .as_mut()
        .ok_or_else(|| AppError::Message("buffer allocation missing".to_string()))?;
    let mapped = allocation
        .mapped_slice_mut()
        .ok_or_else(|| AppError::Message("buffer is not host visible".to_string()))?;
    if data.len() > mapped.len() {
        return Err(AppError::Message(format!(
            "buffer upload overflow: {} > {}",
            data.len(),
            mapped.len()
        )));
    }
    mapped[..data.len()].copy_from_slice(data);
    Ok(())
}

fn read_bytes(buffer: &AllocatedBuffer, byte_len: usize) -> Result<Vec<u8>, AppError> {
    let allocation = buffer
        .allocation
        .as_ref()
        .ok_or_else(|| AppError::Message("buffer allocation missing".to_string()))?;
    let mapped = allocation
        .mapped_slice()
        .ok_or_else(|| AppError::Message("buffer is not host visible".to_string()))?;
    if byte_len > mapped.len() {
        return Err(AppError::Message(format!(
            "buffer read overflow: {} > {}",
            byte_len,
            mapped.len()
        )));
    }

    Ok(mapped[..byte_len].to_vec())
}

fn rgba_image_byte_len(extent: vk::Extent2D) -> Result<usize, AppError> {
    let width = usize::try_from(extent.width)
        .map_err(|_| AppError::Message("image width did not fit in usize".to_string()))?;
    let height = usize::try_from(extent.height)
        .map_err(|_| AppError::Message("image height did not fit in usize".to_string()))?;

    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| AppError::Message("RGBA image byte size overflowed".to_string()))
}

fn write_png(path: &Path, extent: vk::Extent2D, rgba_bytes: &[u8]) -> Result<(), AppError> {
    let expected_len = rgba_image_byte_len(extent)?;
    if rgba_bytes.len() != expected_len {
        return Err(AppError::Message(format!(
            "PNG data length {} did not match expected RGBA byte length {}",
            rgba_bytes.len(),
            expected_len
        )));
    }

    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, extent.width, extent.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| AppError::Message(format!("failed to write PNG header: {error}")))?;
    writer
        .write_image_data(rgba_bytes)
        .map_err(|error| AppError::Message(format!("failed to write PNG data: {error}")))?;

    Ok(())
}

fn destroy_buffer_with(
    device: &Device,
    allocator: &mut Allocator,
    buffer: &mut AllocatedBuffer,
) -> Result<(), AppError> {
    if buffer.buffer != vk::Buffer::null() {
        unsafe { device.destroy_buffer(buffer.buffer, None) };
        buffer.buffer = vk::Buffer::null();
    }
    if let Some(allocation) = buffer.allocation.take() {
        allocator
            .free(allocation)
            .map_err(|error| AppError::Message(error.to_string()))?;
    }
    Ok(())
}

fn destroy_image_with(
    device: &Device,
    allocator: &mut Allocator,
    image: &mut AllocatedImage,
) -> Result<(), AppError> {
    if image.view != vk::ImageView::null() {
        unsafe { device.destroy_image_view(image.view, None) };
        image.view = vk::ImageView::null();
    }
    if image.image != vk::Image::null() {
        unsafe { device.destroy_image(image.image, None) };
        image.image = vk::Image::null();
    }
    if let Some(allocation) = image.allocation.take() {
        allocator
            .free(allocation)
            .map_err(|error| AppError::Message(error.to_string()))?;
    }
    image.layout = vk::ImageLayout::UNDEFINED;
    Ok(())
}

fn transition_image(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    src_stage: vk::PipelineStageFlags,
    dst_stage: vk::PipelineStageFlags,
    src_access: vk::AccessFlags,
    dst_access: vk::AccessFlags,
) {
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[vk::ImageMemoryBarrier::default()
                .old_layout(old_layout)
                .new_layout(new_layout)
                .src_access_mask(src_access)
                .dst_access_mask(dst_access)
                .image(image)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1),
                )],
        );
    }
}

const fn align_up(value: usize, alignment: usize) -> usize {
    if alignment <= 1 {
        value
    } else {
        value.div_ceil(alignment) * alignment
    }
}

unsafe extern "system" fn debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    let message = if callback_data.is_null() {
        Cow::Borrowed("unknown Vulkan validation message")
    } else {
        unsafe { CStr::from_ptr((*callback_data).p_message) }.to_string_lossy()
    };

    if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        warn!("Vulkan validation error: {message}");
    } else {
        warn!("Vulkan validation: {message}");
    }

    vk::FALSE
}

#[cfg(test)]
mod tests {
    use ash::vk;

    use super::rgba_image_byte_len;

    #[test]
    fn rgba_image_byte_len_matches_extent() {
        let byte_len = rgba_image_byte_len(vk::Extent2D {
            width: 1280,
            height: 720,
        })
        .expect("byte length should fit");

        assert_eq!(byte_len, 1280 * 720 * 4);
    }

    #[test]
    fn rgba_image_byte_len_detects_overflow() {
        let error = rgba_image_byte_len(vk::Extent2D {
            width: u32::MAX,
            height: u32::MAX,
        })
        .expect_err("overflow should be reported");

        assert!(error.to_string().contains("overflowed"));
    }
}
