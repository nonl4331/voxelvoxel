use std::{
    error::Error,
    ffi::{c_char, CStr},
};

use winit::{
    event_loop::EventLoop,
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
    window::WindowBuilder,
};

use ash::vk;

const unsafe fn cstr(a: &'static str) -> &std::ffi::CStr {
    std::ffi::CStr::from_bytes_with_nul_unchecked(a.as_bytes())
}

const LAYER_NAMES: [*const c_char; 1] = unsafe { [cstr("VK_LAYER_KHRONOS_validation\0").as_ptr()] };
const DEVICE_EXTENSION_NAMES: [*const c_char; 5] = unsafe {
    [
        cstr("VK_KHR_ray_tracing_pipeline\0").as_ptr(),
        cstr("VK_KHR_spirv_1_4\0").as_ptr(),
        cstr("VK_KHR_acceleration_structure\0").as_ptr(),
        cstr("VK_KHR_deferred_host_operations\0").as_ptr(),
        ash::khr::swapchain::NAME.as_ptr(),
    ]
};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

struct VoxelRenderer {
    instance: ash::Instance,
    entry: ash::Entry,
    event_loop: EventLoop<()>,
    window: Window,
    debug_callback: vk::DebugUtilsMessengerEXT,
}

impl VoxelRenderer {
    unsafe fn create_instance(
        entry: &ash::Entry,
        window: &Window,
    ) -> Result<ash::Instance, Box<dyn Error>> {
        let appinfo = vk::ApplicationInfo::default()
            .application_name(cstr("VoxelVoxel\0"))
            .application_version(0)
            .engine_name(cstr("No Engine\0"))
            .api_version(vk::make_api_version(0, 1, 2, 162));

        let mut extension_names =
            ash_window::enumerate_required_extensions(window.display_handle()?.as_raw())
                .unwrap()
                .to_vec();

        extension_names.extend_from_slice(&[ash::ext::debug_utils::NAME.as_ptr()]);

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&appinfo)
            .enabled_layer_names(&LAYER_NAMES)
            .enabled_extension_names(&extension_names);

        Ok(entry.create_instance(&create_info, None)?)
    }
    unsafe fn setup_debug_callback(
        entry: &ash::Entry,
        instance: &ash::Instance,
    ) -> Result<vk::DebugUtilsMessengerEXT, Box<dyn Error>> {
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                    | vk::DebugUtilsMessageTypeFlagsEXT::DEVICE_ADDRESS_BINDING,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));

        let debug_utils_loader = ash::ext::debug_utils::Instance::new(entry, instance);
        Ok(debug_utils_loader.create_debug_utils_messenger(&debug_info, None)?)
    }
    pub unsafe fn find_suitable_physical_device(
        instance: &ash::Instance,
        surface: &vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
    ) -> Result<(vk::PhysicalDevice, u32), Box<dyn Error>> {
        // for now until actual requirements,
        // for presentation are figured out
        let queue_family_supports_features = |info: &vk::QueueFamilyProperties,
                                              physical_device: &vk::PhysicalDevice,
                                              index: u32|
         -> Option<()> {
            if info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                && surface_loader
                    .get_physical_device_surface_support(*physical_device, index, *surface)
                    .ok()?
            {
                return Some(());
            }
            None
        };

        instance
            .enumerate_physical_devices()?
            .iter()
            .find_map(|physical_device| {
                let exts = instance
                    .enumerate_device_extension_properties(*physical_device)
                    .ok()?;

                let _ = exts.into_iter().find_map(|ext| {
                    Some(
                        CStr::from_ptr(DEVICE_EXTENSION_NAMES[0])
                            == CStr::from_bytes_until_nul(&ext.extension_name.map(|v| v as u8))
                                .ok()?,
                    )
                })?;

                instance
                    .get_physical_device_queue_family_properties(*physical_device)
                    .iter()
                    .enumerate()
                    .find_map(|(index, info)| {
                        queue_family_supports_features(info, physical_device, index as u32)
                            .map(|_| (*physical_device, index as u32))
                    })
            })
            .ok_or("No suitable physical devices found.".into())
    }
    pub unsafe fn create_queue_and_logical_device(
        instance: &ash::Instance,
        physical_device: &vk::PhysicalDevice,
        queue_family_index: u32,
    ) -> Result<(ash::Device, vk::Queue), Box<dyn Error>> {
        let queue_priorities = [1.0];
        // note queue count is queue_priorities.len()
        let queue_create_infos = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities)];

        let device_features = vk::PhysicalDeviceFeatures::default();

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&DEVICE_EXTENSION_NAMES)
            .enabled_features(&device_features);

        let device = instance.create_device(*physical_device, &device_create_info, None)?;

        let present_queue = device.get_device_queue(queue_family_index, 0);
        Ok((device, present_queue))
    }
    pub unsafe fn create_swapchain(
        instance: &ash::Instance,
        physical_device: &vk::PhysicalDevice,
        device: &ash::Device,
        surface: &vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
    ) -> Result<
        (
            vk::SwapchainKHR,
            ash::khr::swapchain::Device,
            vk::SurfaceFormatKHR,
        ),
        Box<dyn Error>,
    > {
        let capabilities =
            surface_loader.get_physical_device_surface_capabilities(*physical_device, *surface)?;

        let formats =
            surface_loader.get_physical_device_surface_formats(*physical_device, *surface)?;

        let present_modes =
            surface_loader.get_physical_device_surface_present_modes(*physical_device, *surface)?;

        let present_mode = if present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else {
            *present_modes.first().ok_or("No present modes detected!")?
        };

        let mut image_count = capabilities.min_image_count + 1;
        // max_image_count of 0 means unlimited
        if capabilities.max_image_count > 0 && image_count > capabilities.max_image_count {
            image_count = capabilities.max_image_count;
        }

        let extent = if capabilities.current_extent.width == u32::MAX {
            vk::Extent2D {
                width: WIDTH,
                height: HEIGHT,
            }
        } else {
            capabilities.current_extent
        };

        let pre_transform = if capabilities
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            capabilities.current_transform
        };

        let format = formats.first().ok_or("No formats detected!")?;

        let swapchain_loader = ash::khr::swapchain::Device::new(instance, device);

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(*surface)
            .min_image_count(image_count)
            .image_color_space(format.color_space)
            .image_format(format.format)
            .image_extent(extent)
            // image usage & sharing mode might need to change
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .image_array_layers(1)
            .present_mode(present_mode)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .pre_transform(pre_transform)
            .clipped(true);

        Ok((
            swapchain_loader.create_swapchain(&swapchain_create_info, None)?,
            swapchain_loader,
            *format,
        ))
    }
    unsafe fn get_swapchain_images(
        device: &ash::Device,
        swapchain: &vk::SwapchainKHR,
        swapchain_loader: &ash::khr::swapchain::Device,
        format: vk::SurfaceFormatKHR,
    ) -> Result<(Vec<vk::Image>, Vec<vk::ImageView>), Box<dyn Error>> {
        let images = swapchain_loader.get_swapchain_images(*swapchain)?;

        let image_views: Result<Vec<_>, ash::vk::Result> = images
            .iter()
            .map(|&image| {
                let swizzle_ident = vk::ComponentSwizzle::IDENTITY;
                let image_view_create_info = vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format.format)
                    .components(vk::ComponentMapping {
                        r: swizzle_ident,
                        g: swizzle_ident,
                        b: swizzle_ident,
                        a: vk::ComponentSwizzle::ONE,
                    })
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });

                device.create_image_view(&image_view_create_info, None)
            })
            .collect();

        Ok((images, image_views?))
    }
    pub fn new(win_width: u32, win_height: u32) -> Result<Self, Box<dyn Error>> {
        unsafe {
            // loads entry points from a vulkan loader at compile time
            let entry = ash::Entry::linked();

            let event_loop = EventLoop::new()?;
            let window = WindowBuilder::new()
                .with_title("Voxel Renderer")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    win_width as f64,
                    win_height as f64,
                ))
                .build(&event_loop)?;

            let instance = Self::create_instance(&entry, &window)?;

            let debug_callback = Self::setup_debug_callback(&entry, &instance)?;

            let surface = ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle()?.as_raw(),
                window.window_handle()?.as_raw(),
                None,
            )?;
            let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

            let (physical_device, queue_family_index) =
                Self::find_suitable_physical_device(&instance, &surface, &surface_loader)?;

            log::info!(
                "Physical Device chosen: {:?}",
                CStr::from_bytes_until_nul(
                    &instance
                        .get_physical_device_properties(physical_device)
                        .device_name
                        .map(|v| v as u8)
                )?
            );

            let (logical_device, present_queue) = Self::create_queue_and_logical_device(
                &instance,
                &physical_device,
                queue_family_index,
            )?;

            let (swapchain, swapchain_loader, format) = Self::create_swapchain(
                &instance,
                &physical_device,
                &logical_device,
                &surface,
                &surface_loader,
            )?;

            let (images, image_views) =
                Self::get_swapchain_images(&logical_device, &swapchain, &swapchain_loader, format)?;

            Ok(Self {
                entry,
                event_loop,
                window,
                instance,
                debug_callback,
            })
        }
    }
}

fn main() {
    env_logger::init();

    VoxelRenderer::new(800, 600).unwrap();
}

unsafe extern "system" fn vulkan_debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    msg_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;

    type Type = vk::DebugUtilsMessageTypeFlagsEXT;
    type Severity = vk::DebugUtilsMessageSeverityFlagsEXT;

    macro_rules! contains {
        ($a:expr, $b:tt, $c:expr) => {
            if $a.contains(Type::$b) {
                $c
            } else {
                '_'
            }
        };
    }

    let g = contains!(msg_type, GENERAL, 'G');
    let v = contains!(msg_type, VALIDATION, 'V');
    let p = contains!(msg_type, PERFORMANCE, 'P');
    let b = contains!(msg_type, DEVICE_ADDRESS_BINDING, 'B');
    let message = if b == 'B' {
        format!(
            "{g}{v}{p} | {:?}",
            CStr::from_ptr(callback_data.p_message_id_name)
        )
    } else {
        format!(
            "{g}{v}{p}{b} | {:?} | {:?}",
            CStr::from_ptr(callback_data.p_message_id_name),
            CStr::from_ptr(callback_data.p_message)
        )
    };

    if severity.contains(Severity::ERROR) {
        log::error!("{message}");
    } else if severity.contains(Severity::WARNING) {
        log::warn!("{message}");
    } else if severity.contains(Severity::INFO) {
        log::info!("{message}");
    } else if severity.contains(Severity::VERBOSE) {
        log::debug!("{message}");
    } else {
        log::trace!("{message}");
    }

    vk::FALSE
}
