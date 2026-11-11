//! Vulkan backend (ash). Shaders are written in WGSL and compiled to SPIR-V at
//! startup with naga, so the build needs no external shader compiler.

use super::atlas::{Atlas, ATLAS_SIZE, MIP_LEVELS};
use super::mesher::{ChunkMesh, Vertex};
use super::Mat4;
use ash::vk;
use std::collections::HashMap;
use std::ffi::CStr;

const SHADER: &str = r#"
struct Globals {
    view_proj: mat4x4<f32>,
    sky_rot: mat4x4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_fwd: vec4<f32>,
    fog_color: vec4<f32>,
    sky_color: vec4<f32>,
    fog_params: vec4<f32>,
    sun_dir: vec4<f32>,
    sky_params: vec4<f32>,
    sun_rect: vec4<f32>,
    moon_rect: vec4<f32>,
    void_color: vec4<f32>,
    lightmap: array<vec4<f32>, 256>,
}
struct Push {
    offset: vec4<f32>,
    color: vec4<f32>,
}
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
var<push_constant> pc: Push;

struct TerrainOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) light: vec2<f32>,
    @location(3) rel: vec3<f32>,
}

@vertex
fn vs_terrain(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>, @location(3) light: vec4<f32>) -> TerrainOut {
    var o: TerrainOut;
    let rel = pos + pc.offset.xyz;
    o.pos = g.view_proj * vec4<f32>(rel, 1.0);
    o.uv = uv;
    o.color = color;
    o.light = light.xy * (255.0 / 16.0);
    o.rel = rel;
    return o;
}

fn fog_factor(d: f32) -> f32 {
    if (g.fog_params.w > 0.5) {
        return clamp(exp(-g.fog_params.z * d), 0.0, 1.0);
    }
    return clamp((g.fog_params.y - d) / (g.fog_params.y - g.fog_params.x), 0.0, 1.0);
}

@fragment
fn fs_terrain(i: TerrainOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    if (t.a * i.color.a <= 0.1) {
        discard;
    }
    let lb = clamp(i.light.x, 0.0, 15.0);
    let ls = clamp(i.light.y, 0.0, 15.0);
    let b0 = u32(floor(lb));
    let s0 = u32(floor(ls));
    let b1 = min(b0 + 1u, 15u);
    let s1 = min(s0 + 1u, 15u);
    let fb = lb - floor(lb);
    let fs = ls - floor(ls);
    let lm = mix(mix(g.lightmap[s0 * 16u + b0], g.lightmap[s0 * 16u + b1], fb),
                 mix(g.lightmap[s1 * 16u + b0], g.lightmap[s1 * 16u + b1], fb), fs);
    let c = t.rgb * i.color.rgb * lm.rgb;
    let f = fog_factor(length(i.rel));
    return vec4<f32>(mix(g.fog_color.rgb, c, f), t.a * i.color.a);
}

struct SkyOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    var o: SkyOut;
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u)) * 2.0 - 1.0;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let dir = normalize(g.cam_fwd.xyz + g.cam_right.xyz * i.ndc.x - g.cam_up.xyz * i.ndc.y);
    let fog_end = g.sky_params.x;
    var c = g.fog_color.rgb;
    if (g.fog_params.w > 0.5) {
        return vec4<f32>(c, 1.0);
    }
    if (dir.y > 0.0) {
        let t = 16.0 / dir.y;
        let f = clamp((fog_end - t) / fog_end, 0.0, 1.0);
        c = mix(g.fog_color.rgb, g.sky_color.rgb, f);
        let sd = g.sun_dir.xyz;
        let az = vec3<f32>(-sd.y, sd.x, 0.0);
        let k = dot(dir, sd);
        if (k > 0.001) {
            let p = dir * (100.0 / k);
            let lx = p.z;
            let lz = dot(p, az);
            if (abs(lx) < 30.0 && abs(lz) < 30.0) {
                let uv = vec2<f32>(lx / 60.0 + 0.5, lz / 60.0 + 0.5);
                let s = textureSampleLevel(tex, samp, g.sun_rect.xy + uv * (g.sun_rect.zw - g.sun_rect.xy), 0.0);
                c = c + s.rgb * s.a * g.sky_params.y;
            }
        }
        if (k < -0.001) {
            let p = dir * (100.0 / -k);
            let lx = p.z;
            let lz = dot(p, az);
            if (abs(lx) < 20.0 && abs(lz) < 20.0) {
                let uv = vec2<f32>(0.5 - lx / 40.0, lz / 40.0 + 0.5);
                let s = textureSampleLevel(tex, samp, g.moon_rect.xy + uv * (g.moon_rect.zw - g.moon_rect.xy), 0.0);
                c = c + s.rgb * s.a * g.sky_params.y;
            }
        }
    } else if (g.sun_dir.w > 0.0 && dir.y < 0.0) {
        let t = g.sun_dir.w / -dir.y;
        let f = clamp((fog_end - t) / fog_end, 0.0, 1.0);
        c = mix(g.fog_color.rgb, g.void_color.rgb, f);
    }
    return vec4<f32>(c, 1.0);
}

struct ColorOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) rel: vec3<f32>,
}

@vertex
fn vs_color(@location(0) pos: vec3<f32>, @location(2) color: vec4<f32>) -> ColorOut {
    var o: ColorOut;
    var p = pos + pc.offset.xyz;
    if (pc.offset.w > 1.5) {
        p = (g.sky_rot * vec4<f32>(pos, 1.0)).xyz;
    }
    o.pos = g.view_proj * vec4<f32>(p, 1.0);
    o.color = color * pc.color;
    o.rel = p;
    return o;
}

@fragment
fn fs_color(i: ColorOut) -> @location(0) vec4<f32> {
    return i.color;
}

@fragment
fn fs_color_fog(i: ColorOut) -> @location(0) vec4<f32> {
    let f = fog_factor(length(i.rel));
    return vec4<f32>(mix(g.fog_color.rgb, i.color.rgb, f), i.color.a);
}

@vertex
fn vs_line(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    if (pc.offset.w > 0.5) {
        return vec4<f32>(pos.xy, 0.0, 1.0);
    }
    return g.view_proj * vec4<f32>(pos + pc.offset.xyz, 1.0);
}

@fragment
fn fs_line() -> @location(0) vec4<f32> {
    return pc.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Globals {
    pub view_proj: Mat4,
    /// Celestial rotation applied to the star field.
    pub sky_rot: Mat4,
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    pub cam_fwd: [f32; 4],
    pub fog_color: [f32; 4],
    pub sky_color: [f32; 4],
    /// start, end, density, mode (0 linear, 1 exp)
    pub fog_params: [f32; 4],
    /// xyz sun direction, w = eye height above the horizon
    pub sun_dir: [f32; 4],
    /// sky fog end, sun/moon alpha
    pub sky_params: [f32; 4],
    pub sun_rect: [f32; 4],
    pub moon_rect: [f32; 4],
    pub void_color: [f32; 4],
    pub lightmap: [[f32; 4]; 256],
}

pub struct Frame<'a> {
    pub globals: &'a Globals,
    pub cam: [f64; 3],
    /// Selected block (world coordinates).
    pub outline: Option<[i32; 3]>,
    pub gui_scale: f32,
    pub screenshot: Option<std::path::PathBuf>,
    /// `World.getStarBrightness`
    pub star_brightness: f32,
    /// Sunrise/sunset glow: colour with alpha, and whether it is on the sunset side.
    pub sunrise: Option<([f32; 4], bool)>,
    pub clouds: Option<CloudDraw>,
}

pub struct CloudDraw {
    /// Mesh origin relative to the camera.
    pub offset: [f32; 3],
    pub color: [f32; 3],
    /// Below the cloud layer they are drawn before the terrain.
    pub before_terrain: bool,
}

struct GpuMesh {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    opaque_quads: u32,
    translucent_quads: u32,
    max_y: f32,
}

struct PerFrame {
    cmd: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    fence: vk::Fence,
    ubo: vk::Buffer,
    ubo_mem: vk::DeviceMemory,
    ubo_ptr: *mut u8,
    dynbuf: vk::Buffer,
    dyn_mem: vk::DeviceMemory,
    dyn_ptr: *mut u8,
    /// Coloured dynamic geometry (sunrise glow).
    dyn2: vk::Buffer,
    dyn2_mem: vk::DeviceMemory,
    dyn2_ptr: *mut u8,
    /// Staging area for animated texture uploads.
    stage: vk::Buffer,
    stage_mem: vk::DeviceMemory,
    stage_ptr: *mut u8,
    set: vk::DescriptorSet,
}

/// One animated tile frame waiting to be copied into the atlas.
pub struct TileUpdate {
    pub x: u32,
    pub y: u32,
    pub size: u32,
    pub mips: Vec<Vec<u8>>,
}

const STAGE_SIZE: u64 = 1 << 16;

const FRAMES: usize = 2;
const DYN_SIZE: u64 = 4096;

pub struct Renderer {
    _entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    pdev: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    swap_loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    format: vk::SurfaceFormatKHR,
    pub extent: vk::Extent2D,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    framebuffers: Vec<vk::Framebuffer>,
    depth: (vk::Image, vk::DeviceMemory, vk::ImageView),
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    set_layout: vk::DescriptorSetLayout,
    desc_pool: vk::DescriptorPool,
    pipe_sky: vk::Pipeline,
    pipe_opaque: vk::Pipeline,
    pipe_translucent: vk::Pipeline,
    pipe_line: vk::Pipeline,
    pipe_ui: vk::Pipeline,
    pipe_sky_alpha: vk::Pipeline,
    pipe_sky_add: vk::Pipeline,
    pipe_cloud_depth: vk::Pipeline,
    pipe_cloud_color: vk::Pipeline,
    stars: (vk::Buffer, vk::DeviceMemory, u32),
    cloud_mesh: Option<(vk::Buffer, vk::DeviceMemory, u32)>,
    tile_updates: Vec<TileUpdate>,
    pool: vk::CommandPool,
    frames: Vec<PerFrame>,
    frame_index: usize,
    frame_no: u64,
    atlas: (vk::Image, vk::DeviceMemory, vk::ImageView),
    sampler: vk::Sampler,
    index_buf: (vk::Buffer, vk::DeviceMemory),
    index_quads: u32,
    meshes: HashMap<(i32, i32), GpuMesh>,
    garbage: Vec<(u64, vk::Buffer, vk::DeviceMemory)>,
    mem_props: vk::PhysicalDeviceMemoryProperties,
    wide_lines: bool,
    needs_recreate: bool,
    pub device_name: String,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
}

/// Messages reported by the validation layer so far.
pub static VALIDATION_MESSAGES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    VALIDATION_MESSAGES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let msg = if data.is_null() || (*data).p_message.is_null() { "".into() } else { CStr::from_ptr((*data).p_message).to_string_lossy() };
    eprintln!("[vulkan {severity:?}] {msg}");
    vk::FALSE
}

fn compile_shader() -> Vec<u32> {
    let module = match naga::front::wgsl::parse_str(SHADER) {
        Ok(m) => m,
        Err(e) => panic!("shader parse error:\n{}", e.emit_to_string(SHADER)),
    };
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("shader validation error: {e:?}"));
    let mut opts = naga::back::spv::Options::default();
    opts.flags = naga::back::spv::WriterFlags::empty();
    naga::back::spv::write_vec(&module, &info, &opts, None).expect("SPIR-V generation failed")
}

impl Renderer {
    fn mem_type(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> u32 {
        find_mem_type(&self.mem_props, bits, flags)
    }

    fn create_buffer(&self, size: u64, usage: vk::BufferUsageFlags, flags: vk::MemoryPropertyFlags) -> (vk::Buffer, vk::DeviceMemory) {
        unsafe {
            let buf = self
                .device
                .create_buffer(&vk::BufferCreateInfo::default().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE), None)
                .unwrap();
            let req = self.device.get_buffer_memory_requirements(buf);
            let mem = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default().allocation_size(req.size).memory_type_index(self.mem_type(req.memory_type_bits, flags)),
                    None,
                )
                .unwrap();
            self.device.bind_buffer_memory(buf, mem, 0).unwrap();
            (buf, mem)
        }
    }

    fn write_buffer(&self, mem: vk::DeviceMemory, data: &[u8]) {
        unsafe {
            let p = self.device.map_memory(mem, 0, data.len() as u64, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
            std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
            self.device.unmap_memory(mem);
        }
    }

    pub fn new(window: &winit::window::Window, atlas: &Atlas) -> Result<Renderer, String> {
        use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
        unsafe {
            let entry = ash::Entry::load().map_err(|e| format!("Vulkan loader not found: {e}"))?;
            let dh = window.display_handle().map_err(|e| e.to_string())?.as_raw();
            let wh = window.window_handle().map_err(|e| e.to_string())?.as_raw();
            let exts = ash_window::enumerate_required_extensions(dh).map_err(|e| e.to_string())?;
            let app = vk::ApplicationInfo::default().application_name(c"Minecraft 1.7.10").api_version(vk::API_VERSION_1_0);
            // Optional Khronos validation (`--validate`): every message it
            // reports is counted and printed.
            let mut exts: Vec<*const std::ffi::c_char> = exts.to_vec();
            let mut layers: Vec<*const std::ffi::c_char> = Vec::new();
            let want_validation = std::env::var_os("MINERUST_VALIDATE").is_some();
            let mut validation = false;
            if want_validation {
                let available = entry.enumerate_instance_layer_properties().unwrap_or_default();
                validation = available.iter().any(|l| CStr::from_ptr(l.layer_name.as_ptr()) == c"VK_LAYER_KHRONOS_validation");
                if validation {
                    layers.push(c"VK_LAYER_KHRONOS_validation".as_ptr());
                    exts.push(ash::ext::debug_utils::NAME.as_ptr());
                    println!("Vulkan validation: enabled");
                } else {
                    println!("Vulkan validation: VK_LAYER_KHRONOS_validation is not installed on this system");
                }
            }
            let instance = entry
                .create_instance(&vk::InstanceCreateInfo::default().application_info(&app).enabled_extension_names(&exts).enabled_layer_names(&layers), None)
                .map_err(|e| format!("create_instance: {e}"))?;
            let debug = if validation {
                let loader = ash::ext::debug_utils::Instance::new(&entry, &instance);
                let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING)
                    .message_type(vk::DebugUtilsMessageTypeFlagsEXT::GENERAL | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE)
                    .pfn_user_callback(Some(debug_callback));
                loader.create_debug_utils_messenger(&info, None).ok().map(|m| (loader, m))
            } else {
                None
            };
            let surface = ash_window::create_surface(&entry, &instance, dh, wh, None).map_err(|e| format!("create_surface: {e}"))?;
            let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

            let mut chosen = None;
            for pd in instance.enumerate_physical_devices().map_err(|e| e.to_string())? {
                let props = instance.get_physical_device_properties(pd);
                for (i, q) in instance.get_physical_device_queue_family_properties(pd).iter().enumerate() {
                    if q.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                        && surface_loader.get_physical_device_surface_support(pd, i as u32, surface).unwrap_or(false)
                    {
                        let score = if props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU { 2 } else { 1 };
                        if chosen.map_or(true, |(_, _, s)| score > s) {
                            chosen = Some((pd, i as u32, score));
                        }
                        break;
                    }
                }
            }
            let (pdev, qf, _) = chosen.ok_or("no Vulkan device can present to this window")?;
            let props = instance.get_physical_device_properties(pdev);
            let device_name = CStr::from_ptr(props.device_name.as_ptr()).to_string_lossy().into_owned();
            let feats = instance.get_physical_device_features(pdev);
            let wide_lines = feats.wide_lines == vk::TRUE;
            let enabled = vk::PhysicalDeviceFeatures::default().wide_lines(wide_lines);
            let prio = [1.0f32];
            let qci = [vk::DeviceQueueCreateInfo::default().queue_family_index(qf).queue_priorities(&prio)];
            let dev_exts = [ash::khr::swapchain::NAME.as_ptr()];
            let device = instance
                .create_device(
                    pdev,
                    &vk::DeviceCreateInfo::default().queue_create_infos(&qci).enabled_extension_names(&dev_exts).enabled_features(&enabled),
                    None,
                )
                .map_err(|e| format!("create_device: {e}"))?;
            let queue = device.get_device_queue(qf, 0);
            let swap_loader = ash::khr::swapchain::Device::new(&instance, &device);
            let mem_props = instance.get_physical_device_memory_properties(pdev);

            let formats = surface_loader.get_physical_device_surface_formats(pdev, surface).map_err(|e| e.to_string())?;
            let format = formats
                .iter()
                .copied()
                .find(|f| f.format == vk::Format::B8G8R8A8_UNORM && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
                .or_else(|| formats.iter().copied().find(|f| f.format == vk::Format::R8G8B8A8_UNORM))
                .unwrap_or(formats[0]);

            // Render pass
            let atts = [
                vk::AttachmentDescription::default()
                    .format(format.format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
                vk::AttachmentDescription::default()
                    .format(vk::Format::D32_SFLOAT)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
            ];
            let color_ref = [vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL }];
            let depth_ref = vk::AttachmentReference { attachment: 1, layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL };
            let subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&color_ref)
                .depth_stencil_attachment(&depth_ref)];
            let deps = [vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)];
            let render_pass = device
                .create_render_pass(&vk::RenderPassCreateInfo::default().attachments(&atts).subpasses(&subpasses).dependencies(&deps), None)
                .map_err(|e| e.to_string())?;

            // Descriptors
            let bindings = [
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(1)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                vk::DescriptorSetLayoutBinding::default()
                    .binding(2)
                    .descriptor_type(vk::DescriptorType::SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            ];
            let set_layout =
                device.create_descriptor_set_layout(&vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings), None).map_err(|e| e.to_string())?;
            let set_layouts = [set_layout];
            let pc_ranges =
                [vk::PushConstantRange { stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT, offset: 0, size: 32 }];
            let layout = device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts).push_constant_ranges(&pc_ranges), None)
                .map_err(|e| e.to_string())?;
            let pool_sizes = [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: FRAMES as u32 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: FRAMES as u32 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: FRAMES as u32 },
            ];
            let desc_pool = device
                .create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(FRAMES as u32).pool_sizes(&pool_sizes), None)
                .map_err(|e| e.to_string())?;

            let pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(qf).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(|e| e.to_string())?;

            // Pipelines
            let spv = compile_shader();
            let module = device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spv), None).map_err(|e| e.to_string())?;
            let mk = |vs: &CStr, fs: &CStr, kind: u32| -> vk::Pipeline {
                // kind: 0 sky, 1 opaque, 2 translucent, 3 line, 4 ui
                let stages = [
                    vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::VERTEX).module(module).name(vs),
                    vk::PipelineShaderStageCreateInfo::default().stage(vk::ShaderStageFlags::FRAGMENT).module(module).name(fs),
                ];
                // kind: 5 sky alpha, 6 sky additive, 7 cloud depth, 8 cloud colour
                let stride = if kind == 3 || kind == 4 { 12 } else { std::mem::size_of::<Vertex>() as u32 };
                let va_color = [
                    vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32B32_SFLOAT, offset: 0 },
                    vk::VertexInputAttributeDescription { location: 2, binding: 0, format: vk::Format::R8G8B8A8_UNORM, offset: 20 },
                ];
                let vb = [vk::VertexInputBindingDescription { binding: 0, stride, input_rate: vk::VertexInputRate::VERTEX }];
                let va_terrain = [
                    vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32B32_SFLOAT, offset: 0 },
                    vk::VertexInputAttributeDescription { location: 1, binding: 0, format: vk::Format::R32G32_SFLOAT, offset: 12 },
                    vk::VertexInputAttributeDescription { location: 2, binding: 0, format: vk::Format::R8G8B8A8_UNORM, offset: 20 },
                    vk::VertexInputAttributeDescription { location: 3, binding: 0, format: vk::Format::R8G8B8A8_UNORM, offset: 24 },
                ];
                let mut vi = vk::PipelineVertexInputStateCreateInfo::default();
                if kind == 1 || kind == 2 {
                    vi = vi.vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&va_terrain);
                } else if kind == 3 || kind == 4 {
                    vi = vi.vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&va_terrain[..1]);
                } else if kind >= 5 {
                    vi = vi.vertex_binding_descriptions(&vb).vertex_attribute_descriptions(&va_color);
                }
                let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
                    .topology(if kind == 3 { vk::PrimitiveTopology::LINE_LIST } else { vk::PrimitiveTopology::TRIANGLE_LIST });
                let vp = vk::PipelineViewportStateCreateInfo::default().viewport_count(1).scissor_count(1);
                let rs = vk::PipelineRasterizationStateCreateInfo::default()
                    .polygon_mode(vk::PolygonMode::FILL)
                    .cull_mode(if kind == 1 || kind == 2 { vk::CullModeFlags::BACK } else { vk::CullModeFlags::NONE })
                    .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                    .line_width(1.0);
                let ms = vk::PipelineMultisampleStateCreateInfo::default().rasterization_samples(vk::SampleCountFlags::TYPE_1);
                let ds = vk::PipelineDepthStencilStateCreateInfo::default()
                    .depth_test_enable((1..=3).contains(&kind) || kind == 7 || kind == 8)
                    .depth_write_enable(kind == 1 || kind == 2 || kind == 7)
                    .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
                let mut att = vk::PipelineColorBlendAttachmentState::default().color_write_mask(vk::ColorComponentFlags::RGBA);
                if kind == 7 {
                    att = att.color_write_mask(vk::ColorComponentFlags::empty());
                }
                if kind == 6 {
                    att = att
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                        .alpha_blend_op(vk::BlendOp::ADD);
                }
                if kind == 2 || kind == 3 || kind == 5 || kind == 8 {
                    att = att
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .alpha_blend_op(vk::BlendOp::ADD);
                } else if kind == 4 {
                    att = att
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::ONE_MINUS_DST_COLOR)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_COLOR)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                        .alpha_blend_op(vk::BlendOp::ADD);
                }
                let atts = [att];
                let cb = vk::PipelineColorBlendStateCreateInfo::default().attachments(&atts);
                let dyn_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR, vk::DynamicState::LINE_WIDTH];
                let dy = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dyn_states);
                let info = vk::GraphicsPipelineCreateInfo::default()
                    .stages(&stages)
                    .vertex_input_state(&vi)
                    .input_assembly_state(&ia)
                    .viewport_state(&vp)
                    .rasterization_state(&rs)
                    .multisample_state(&ms)
                    .depth_stencil_state(&ds)
                    .color_blend_state(&cb)
                    .dynamic_state(&dy)
                    .layout(layout)
                    .render_pass(render_pass)
                    .subpass(0);
                device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None).expect("pipeline creation failed")[0]
            };
            let pipe_sky = mk(c"vs_sky", c"fs_sky", 0);
            let pipe_opaque = mk(c"vs_terrain", c"fs_terrain", 1);
            let pipe_translucent = mk(c"vs_terrain", c"fs_terrain", 2);
            let pipe_line = mk(c"vs_line", c"fs_line", 3);
            let pipe_ui = mk(c"vs_line", c"fs_line", 4);
            let pipe_sky_alpha = mk(c"vs_color", c"fs_color", 5);
            let pipe_sky_add = mk(c"vs_color", c"fs_color", 6);
            let pipe_cloud_depth = mk(c"vs_color", c"fs_color_fog", 7);
            let pipe_cloud_color = mk(c"vs_color", c"fs_color_fog", 8);
            device.destroy_shader_module(module, None);

            let sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .max_lod((MIP_LEVELS - 1) as f32),
                    None,
                )
                .map_err(|e| e.to_string())?;

            let mut r = Renderer {
                _entry: entry,
                instance,
                surface_loader,
                surface,
                pdev,
                device,
                queue,
                swap_loader,
                swapchain: vk::SwapchainKHR::null(),
                format,
                extent: vk::Extent2D::default(),
                images: Vec::new(),
                views: Vec::new(),
                framebuffers: Vec::new(),
                depth: (vk::Image::null(), vk::DeviceMemory::null(), vk::ImageView::null()),
                render_pass,
                layout,
                set_layout,
                desc_pool,
                pipe_sky,
                pipe_opaque,
                pipe_translucent,
                pipe_line,
                pipe_ui,
                pipe_sky_alpha,
                pipe_sky_add,
                pipe_cloud_depth,
                pipe_cloud_color,
                stars: (vk::Buffer::null(), vk::DeviceMemory::null(), 0),
                cloud_mesh: None,
                tile_updates: Vec::new(),
                pool,
                frames: Vec::new(),
                frame_index: 0,
                frame_no: 0,
                atlas: (vk::Image::null(), vk::DeviceMemory::null(), vk::ImageView::null()),
                sampler,
                index_buf: (vk::Buffer::null(), vk::DeviceMemory::null()),
                index_quads: 0,
                meshes: HashMap::new(),
                garbage: Vec::new(),
                mem_props,
                wide_lines,
                needs_recreate: false,
                device_name,
                debug,
            };
            r.upload_atlas(atlas);
            r.ensure_indices(65536);
            r.create_stars();
            let size = window.inner_size();
            r.create_swapchain(size.width, size.height);

            // Per-frame resources
            let cmds = r
                .device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default().command_pool(r.pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(FRAMES as u32),
                )
                .map_err(|e| e.to_string())?;
            let layouts = [r.set_layout; FRAMES];
            let sets = r
                .device
                .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(r.desc_pool).set_layouts(&layouts))
                .map_err(|e| e.to_string())?;
            let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            for i in 0..FRAMES {
                let ubo_size = std::mem::size_of::<Globals>() as u64;
                let (ubo, ubo_mem) = r.create_buffer(ubo_size, vk::BufferUsageFlags::UNIFORM_BUFFER, host);
                let ubo_ptr = r.device.map_memory(ubo_mem, 0, ubo_size, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
                let (dynbuf, dyn_mem) = r.create_buffer(DYN_SIZE, vk::BufferUsageFlags::VERTEX_BUFFER, host);
                let dyn_ptr = r.device.map_memory(dyn_mem, 0, DYN_SIZE, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
                let (dyn2, dyn2_mem) = r.create_buffer(DYN_SIZE, vk::BufferUsageFlags::VERTEX_BUFFER, host);
                let dyn2_ptr = r.device.map_memory(dyn2_mem, 0, DYN_SIZE, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
                let (stage, stage_mem) = r.create_buffer(STAGE_SIZE, vk::BufferUsageFlags::TRANSFER_SRC, host);
                let stage_ptr = r.device.map_memory(stage_mem, 0, STAGE_SIZE, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
                let bi = [vk::DescriptorBufferInfo { buffer: ubo, offset: 0, range: ubo_size }];
                let ii = [vk::DescriptorImageInfo { sampler: vk::Sampler::null(), image_view: r.atlas.2, image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL }];
                let si = [vk::DescriptorImageInfo { sampler: r.sampler, image_view: vk::ImageView::null(), image_layout: vk::ImageLayout::UNDEFINED }];
                let writes = [
                    vk::WriteDescriptorSet::default().dst_set(sets[i]).dst_binding(0).descriptor_type(vk::DescriptorType::UNIFORM_BUFFER).buffer_info(&bi),
                    vk::WriteDescriptorSet::default().dst_set(sets[i]).dst_binding(1).descriptor_type(vk::DescriptorType::SAMPLED_IMAGE).image_info(&ii),
                    vk::WriteDescriptorSet::default().dst_set(sets[i]).dst_binding(2).descriptor_type(vk::DescriptorType::SAMPLER).image_info(&si),
                ];
                r.device.update_descriptor_sets(&writes, &[]);
                let sem = vk::SemaphoreCreateInfo::default();
                r.frames.push(PerFrame {
                    cmd: cmds[i],
                    image_available: r.device.create_semaphore(&sem, None).unwrap(),
                    render_finished: r.device.create_semaphore(&sem, None).unwrap(),
                    fence: r.device.create_fence(&vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED), None).unwrap(),
                    ubo,
                    ubo_mem,
                    ubo_ptr,
                    dynbuf,
                    dyn_mem,
                    dyn_ptr,
                    dyn2,
                    dyn2_mem,
                    dyn2_ptr,
                    stage,
                    stage_mem,
                    stage_ptr,
                    set: sets[i],
                });
            }
            Ok(r)
        }
    }

    fn one_shot(&self, f: impl FnOnce(vk::CommandBuffer)) {
        unsafe {
            let cmd = self
                .device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default().command_pool(self.pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1),
                )
                .unwrap()[0];
            self.device
                .begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
                .unwrap();
            f(cmd);
            self.device.end_command_buffer(cmd).unwrap();
            let cmds = [cmd];
            self.device.queue_submit(self.queue, &[vk::SubmitInfo::default().command_buffers(&cmds)], vk::Fence::null()).unwrap();
            self.device.queue_wait_idle(self.queue).unwrap();
            self.device.free_command_buffers(self.pool, &cmds);
        }
    }

    fn upload_atlas(&mut self, atlas: &Atlas) {
        unsafe {
            let info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::R8G8B8A8_UNORM)
                .extent(vk::Extent3D { width: ATLAS_SIZE, height: ATLAS_SIZE, depth: 1 })
                .mip_levels(MIP_LEVELS)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let image = self.device.create_image(&info, None).unwrap();
            let req = self.device.get_image_memory_requirements(image);
            let mem = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)),
                    None,
                )
                .unwrap();
            self.device.bind_image_memory(image, mem, 0).unwrap();
            let mut all = Vec::new();
            let mut offsets = Vec::new();
            for m in &atlas.mips {
                offsets.push(all.len() as u64);
                all.extend_from_slice(m);
            }
            let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            let (sb, sm) = self.create_buffer(all.len() as u64, vk::BufferUsageFlags::TRANSFER_SRC, host);
            self.write_buffer(sm, &all);
            let range = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: MIP_LEVELS, base_array_layer: 0, layer_count: 1 };
            let dev = &self.device;
            self.one_shot(|cmd| {
                let b1 = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)];
                dev.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &b1);
                let regions: Vec<vk::BufferImageCopy> = (0..MIP_LEVELS)
                    .map(|l| {
                        vk::BufferImageCopy::default()
                            .buffer_offset(offsets[l as usize])
                            .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: l, base_array_layer: 0, layer_count: 1 })
                            .image_extent(vk::Extent3D { width: ATLAS_SIZE >> l, height: ATLAS_SIZE >> l, depth: 1 })
                    })
                    .collect();
                dev.cmd_copy_buffer_to_image(cmd, sb, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions);
                let b2 = [vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(range)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)];
                dev.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &b2);
            });
            self.device.destroy_buffer(sb, None);
            self.device.free_memory(sm, None);
            let view = self
                .device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default().image(image).view_type(vk::ImageViewType::TYPE_2D).format(vk::Format::R8G8B8A8_UNORM).subresource_range(range),
                    None,
                )
                .unwrap();
            self.atlas = (image, mem, view);
        }
    }

    /// `RenderGlobal.renderStars`: 1500 candidate quads from `Random(10842)`.
    fn create_stars(&mut self) {
        let mut r = crate::java::JavaRandom::new(10842);
        let mut verts: Vec<Vertex> = Vec::new();
        for _ in 0..1500 {
            let mut x = (r.next_float() * 2.0f32 - 1.0f32) as f64;
            let mut y = (r.next_float() * 2.0f32 - 1.0f32) as f64;
            let mut z = (r.next_float() * 2.0f32 - 1.0f32) as f64;
            let size = (0.15f32 + r.next_float() * 0.1f32) as f64;
            let mut d = x * x + y * y + z * z;
            if d < 1.0 && d > 0.01 {
                d = 1.0 / d.sqrt();
                x *= d;
                y *= d;
                z *= d;
                let (px, py, pz) = (x * 100.0, y * 100.0, z * 100.0);
                let a = x.atan2(z);
                let (sa, ca) = (a.sin(), a.cos());
                let b = (x * x + z * z).sqrt().atan2(y);
                let (sb, cb) = (b.sin(), b.cos());
                let rot = r.next_double() * std::f64::consts::PI * 2.0;
                let (sr, cr) = (rot.sin(), rot.cos());
                for k in 0..4i32 {
                    let e = ((k & 2) - 1) as f64 * size;
                    let f = (((k + 1) & 2) - 1) as f64 * size;
                    let g = e * cr - f * sr;
                    let h = f * cr + e * sr;
                    let i2 = g * sb;
                    let j = -g * cb;
                    let vx = j * sa - h * ca;
                    let vz = h * sa + j * ca;
                    verts.push(Vertex { pos: [(px + vx) as f32, (py + i2) as f32, (pz + vz) as f32], uv: [0.0; 2], color: [255; 4], light: [0; 4] });
                }
            }
        }
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let bytes = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * std::mem::size_of::<Vertex>()) };
        let (buf, mem) = self.create_buffer(bytes.len() as u64, vk::BufferUsageFlags::VERTEX_BUFFER, host);
        self.write_buffer(mem, bytes);
        self.stars = (buf, mem, (verts.len() / 4) as u32);
    }

    /// Replaces the cloud layer mesh (quads in `Vertex` format).
    pub fn set_clouds(&mut self, verts: &[Vertex]) {
        if let Some((b, m, _)) = self.cloud_mesh.take() {
            self.garbage.push((self.frame_no, b, m));
        }
        if verts.is_empty() {
            return;
        }
        self.ensure_indices((verts.len() / 4) as u32);
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let bytes = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const u8, verts.len() * std::mem::size_of::<Vertex>()) };
        let (buf, mem) = self.create_buffer(bytes.len() as u64, vk::BufferUsageFlags::VERTEX_BUFFER, host);
        self.write_buffer(mem, bytes);
        self.cloud_mesh = Some((buf, mem, (verts.len() / 4) as u32));
    }

    /// Queues a new frame of an animated tile; applied at the next frame.
    pub fn queue_tile_update(&mut self, u: TileUpdate) {
        // Only the newest frame of a tile matters.
        self.tile_updates.retain(|t| t.x != u.x || t.y != u.y);
        self.tile_updates.push(u);
    }

    fn ensure_indices(&mut self, quads: u32) {
        if quads <= self.index_quads {
            return;
        }
        let quads = quads.next_power_of_two();
        let mut data: Vec<u32> = Vec::with_capacity(quads as usize * 6);
        for q in 0..quads {
            let b = q * 4;
            data.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let (buf, mem) = self.create_buffer(data.len() as u64 * 4, vk::BufferUsageFlags::INDEX_BUFFER, host);
        let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 4) };
        self.write_buffer(mem, bytes);
        if self.index_buf.0 != vk::Buffer::null() {
            self.garbage.push((self.frame_no, self.index_buf.0, self.index_buf.1));
        }
        self.index_buf = (buf, mem);
        self.index_quads = quads;
    }

    fn destroy_swapchain_views(&mut self) {
        unsafe {
            for &f in &self.framebuffers {
                self.device.destroy_framebuffer(f, None);
            }
            for &v in &self.views {
                self.device.destroy_image_view(v, None);
            }
            self.framebuffers.clear();
            self.views.clear();
            if self.depth.0 != vk::Image::null() {
                self.device.destroy_image_view(self.depth.2, None);
                self.device.destroy_image(self.depth.0, None);
                self.device.free_memory(self.depth.1, None);
                self.depth = (vk::Image::null(), vk::DeviceMemory::null(), vk::ImageView::null());
            }
        }
    }

    fn create_swapchain(&mut self, w: u32, h: u32) {
        unsafe {
            self.device.device_wait_idle().unwrap();
            self.destroy_swapchain_views();
            let caps = self.surface_loader.get_physical_device_surface_capabilities(self.pdev, self.surface).unwrap();
            let extent = if caps.current_extent.width != u32::MAX {
                caps.current_extent
            } else {
                vk::Extent2D {
                    width: w.clamp(caps.min_image_extent.width, caps.max_image_extent.width),
                    height: h.clamp(caps.min_image_extent.height, caps.max_image_extent.height),
                }
            };
            self.extent = extent;
            if extent.width == 0 || extent.height == 0 {
                return;
            }
            let mut count = caps.min_image_count + 1;
            if caps.max_image_count > 0 {
                count = count.min(caps.max_image_count);
            }
            let old = self.swapchain;
            let info = vk::SwapchainCreateInfoKHR::default()
                .surface(self.surface)
                .min_image_count(count)
                .image_format(self.format.format)
                .image_color_space(self.format.color_space)
                .image_extent(extent)
                .image_array_layers(1)
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | (caps.supported_usage_flags & vk::ImageUsageFlags::TRANSFER_SRC))
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
                .pre_transform(caps.current_transform)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .present_mode(vk::PresentModeKHR::FIFO)
                .clipped(true)
                .old_swapchain(old);
            self.swapchain = self.swap_loader.create_swapchain(&info, None).unwrap();
            if old != vk::SwapchainKHR::null() {
                self.swap_loader.destroy_swapchain(old, None);
            }
            self.images = self.swap_loader.get_swapchain_images(self.swapchain).unwrap();
            // Depth
            let dinfo = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::D32_SFLOAT)
                .extent(vk::Extent3D { width: extent.width, height: extent.height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT);
            let dimg = self.device.create_image(&dinfo, None).unwrap();
            let req = self.device.get_image_memory_requirements(dimg);
            let dmem = self
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)),
                    None,
                )
                .unwrap();
            self.device.bind_image_memory(dimg, dmem, 0).unwrap();
            let drange = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::DEPTH, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
            let dview = self
                .device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default().image(dimg).view_type(vk::ImageViewType::TYPE_2D).format(vk::Format::D32_SFLOAT).subresource_range(drange),
                    None,
                )
                .unwrap();
            self.depth = (dimg, dmem, dview);
            let crange = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
            for &img in &self.images {
                let v = self
                    .device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default().image(img).view_type(vk::ImageViewType::TYPE_2D).format(self.format.format).subresource_range(crange),
                        None,
                    )
                    .unwrap();
                let atts = [v, dview];
                let fb = self
                    .device
                    .create_framebuffer(
                        &vk::FramebufferCreateInfo::default().render_pass(self.render_pass).attachments(&atts).width(extent.width).height(extent.height).layers(1),
                        None,
                    )
                    .unwrap();
                self.views.push(v);
                self.framebuffers.push(fb);
            }
            self.needs_recreate = false;
        }
    }

    pub fn resize(&mut self) {
        self.needs_recreate = true;
    }

    pub fn has_mesh(&self, key: (i32, i32)) -> bool {
        self.meshes.contains_key(&key)
    }

    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    pub fn remove_mesh(&mut self, key: (i32, i32)) {
        if let Some(m) = self.meshes.remove(&key) {
            if m.buffer != vk::Buffer::null() {
                self.garbage.push((self.frame_no, m.buffer, m.memory));
            }
        }
    }

    pub fn upload_mesh(&mut self, mesh: &ChunkMesh) {
        self.remove_mesh((mesh.cx, mesh.cz));
        let oq = (mesh.opaque.len() / 4) as u32;
        let tq = (mesh.translucent.len() / 4) as u32;
        let mut gm = GpuMesh { buffer: vk::Buffer::null(), memory: vk::DeviceMemory::null(), opaque_quads: oq, translucent_quads: tq, max_y: mesh.max_y };
        if oq + tq > 0 {
            self.ensure_indices(oq.max(tq));
            let vs = std::mem::size_of::<Vertex>();
            let total = (mesh.opaque.len() + mesh.translucent.len()) * vs;
            let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
            let (buf, mem) = self.create_buffer(total as u64, vk::BufferUsageFlags::VERTEX_BUFFER, host);
            unsafe {
                let p = self.device.map_memory(mem, 0, total as u64, vk::MemoryMapFlags::empty()).unwrap() as *mut u8;
                std::ptr::copy_nonoverlapping(mesh.opaque.as_ptr() as *const u8, p, mesh.opaque.len() * vs);
                std::ptr::copy_nonoverlapping(mesh.translucent.as_ptr() as *const u8, p.add(mesh.opaque.len() * vs), mesh.translucent.len() * vs);
                self.device.unmap_memory(mem);
            }
            gm.buffer = buf;
            gm.memory = mem;
        }
        self.meshes.insert((mesh.cx, mesh.cz), gm);
    }

    fn collect_garbage(&mut self) {
        let now = self.frame_no;
        let dev = &self.device;
        self.garbage.retain(|&(f, b, m)| {
            if f + 3 <= now {
                unsafe {
                    dev.destroy_buffer(b, None);
                    dev.free_memory(m, None);
                }
                false
            } else {
                true
            }
        });
    }

    /// Renders one frame. Returns false if nothing was presented.
    pub fn render(&mut self, frame: &Frame, window_size: (u32, u32)) -> bool {
        unsafe {
            if self.needs_recreate || self.extent.width == 0 || self.extent.height == 0 {
                if window_size.0 == 0 || window_size.1 == 0 {
                    return false;
                }
                self.create_swapchain(window_size.0, window_size.1);
                if self.extent.width == 0 || self.extent.height == 0 {
                    return false;
                }
            }
            self.frame_no += 1;
            self.collect_garbage();
            let fi = self.frame_index;
            self.frame_index = (fi + 1) % FRAMES;
            let (cmd, fence, image_available, render_finished, set, dynbuf, dyn_ptr, ubo_ptr, dyn2, dyn2_ptr, stage, stage_ptr) = {
                let f = &self.frames[fi];
                (f.cmd, f.fence, f.image_available, f.render_finished, f.set, f.dynbuf, f.dyn_ptr, f.ubo_ptr, f.dyn2, f.dyn2_ptr, f.stage, f.stage_ptr)
            };
            self.device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            let image_index = match self.swap_loader.acquire_next_image(self.swapchain, u64::MAX, image_available, vk::Fence::null()) {
                Ok((i, sub)) => {
                    if sub {
                        self.needs_recreate = true;
                    }
                    i
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.needs_recreate = true;
                    return false;
                }
                Err(e) => panic!("acquire_next_image: {e}"),
            };
            self.device.reset_fences(&[fence]).unwrap();
            std::ptr::copy_nonoverlapping(frame.globals as *const Globals as *const u8, ubo_ptr, std::mem::size_of::<Globals>());

            // Dynamic geometry: selection box lines + crosshair quads.
            let mut dynv: Vec<[f32; 3]> = Vec::new();
            let mut outline_count = 0u32;
            if let Some(b) = frame.outline {
                let e = 0.002f32;
                let o = [(b[0] as f64 - frame.cam[0]) as f32, (b[1] as f64 - frame.cam[1]) as f32, (b[2] as f64 - frame.cam[2]) as f32];
                let (x0, y0, z0, x1, y1, z1) = (o[0] - e, o[1] - e, o[2] - e, o[0] + 1.0 + e, o[1] + 1.0 + e, o[2] + 1.0 + e);
                let c = [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]];
                for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    dynv.push(c[a]);
                    dynv.push(c[b]);
                }
                outline_count = 24;
            }
            let cross_first = dynv.len() as u32;
            {
                let sx = 2.0 * frame.gui_scale / self.extent.width as f32;
                let sy = 2.0 * frame.gui_scale / self.extent.height as f32;
                let mut rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
                    let (a, b, c, d) = ([x0 * sx, y0 * sy, 0.0], [x1 * sx, y0 * sy, 0.0], [x1 * sx, y1 * sy, 0.0], [x0 * sx, y1 * sy, 0.0]);
                    dynv.extend_from_slice(&[a, b, c, a, c, d]);
                };
                // The 9x9 plus of icons.png, 1 GUI pixel thick.
                rect(-0.5, -4.5, 0.5, 4.5);
                rect(-4.5, -0.5, -0.5, 0.5);
                rect(0.5, -0.5, 4.5, 0.5);
            }
            let cross_count = dynv.len() as u32 - cross_first;
            std::ptr::copy_nonoverlapping(dynv.as_ptr() as *const u8, dyn_ptr, dynv.len() * 12);

            // Visible chunk list (frustum culled, sorted by distance).
            let m = &frame.globals.view_proj;
            let row = |r: usize| [m[r], m[4 + r], m[8 + r], m[12 + r]];
            let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
            let add = |a: [f32; 4], b: [f32; 4], s: f32| [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2], a[3] + s * b[3]];
            let planes = [add(r3, r0, 1.0), add(r3, r0, -1.0), add(r3, r1, 1.0), add(r3, r1, -1.0), r2, add(r3, r2, -1.0)];
            let mut visible: Vec<(f32, [f32; 3], &GpuMesh)> = Vec::new();
            for (&(cx, cz), gm) in &self.meshes {
                if gm.buffer == vk::Buffer::null() {
                    continue;
                }
                let o = [((cx * 16) as f64 - frame.cam[0]) as f32, (0.0 - frame.cam[1]) as f32, ((cz * 16) as f64 - frame.cam[2]) as f32];
                let (mn, mx) = (o, [o[0] + 16.0, o[1] + gm.max_y, o[2] + 16.0]);
                let inside = planes.iter().all(|p| {
                    let x = if p[0] >= 0.0 { mx[0] } else { mn[0] };
                    let y = if p[1] >= 0.0 { mx[1] } else { mn[1] };
                    let z = if p[2] >= 0.0 { mx[2] } else { mn[2] };
                    p[0] * x + p[1] * y + p[2] * z + p[3] >= 0.0
                });
                if inside {
                    let (dx, dz) = (o[0] + 8.0, o[2] + 8.0);
                    visible.push((dx * dx + dz * dz, o, gm));
                }
            }
            visible.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

            // Sunrise / sunset glow: a fan from the zenith of the rotated frame
            // (glRotatef(90, X), glRotatef(0 or 180, Z), glRotatef(90, Z)).
            let mut fan_count = 0u32;
            if let Some((c, flip)) = frame.sunrise {
                let m = super::mul(&super::mul(&super::rot_x(90.0), &super::rot_z(if flip { 180.0 } else { 0.0 })), &super::rot_z(90.0));
                let tr = |p: [f32; 3]| {
                    let o = super::transform(&m, [p[0], p[1], p[2], 1.0]);
                    [o[0], o[1], o[2]]
                };
                let cb = |a: f32| [(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (a * 255.0) as u8];
                let centre = Vertex { pos: tr([0.0, 100.0, 0.0]), uv: [0.0; 2], color: cb(c[3]), light: [0; 4] };
                let rim = |i: i32| {
                    let a = i as f32 * std::f32::consts::PI * 2.0 / 16.0;
                    let (s, co) = (crate::util::math::sin(a), crate::util::math::cos(a));
                    Vertex { pos: tr([s * 120.0, co * 120.0, -co * 40.0 * c[3]]), uv: [0.0; 2], color: cb(0.0), light: [0; 4] }
                };
                let mut fan: Vec<Vertex> = Vec::with_capacity(48);
                for i in 0..16 {
                    fan.extend_from_slice(&[centre, rim(i), rim(i + 1)]);
                }
                std::ptr::copy_nonoverlapping(fan.as_ptr() as *const u8, dyn2_ptr, fan.len() * std::mem::size_of::<Vertex>());
                fan_count = fan.len() as u32;
            }

            let dev = &self.device;
            dev.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty()).unwrap();
            dev.begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default()).unwrap();

            // Animated tiles: copy the new frames into the atlas.
            if !self.tile_updates.is_empty() {
                let mut regions = Vec::new();
                let mut off = 0usize;
                for u in self.tile_updates.drain(..) {
                    let mut s = u.size;
                    for (level, px) in u.mips.iter().enumerate() {
                        if off + px.len() > STAGE_SIZE as usize {
                            break;
                        }
                        std::ptr::copy_nonoverlapping(px.as_ptr(), stage_ptr.add(off), px.len());
                        regions.push(
                            vk::BufferImageCopy::default()
                                .buffer_offset(off as u64)
                                .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: level as u32, base_array_layer: 0, layer_count: 1 })
                                .image_offset(vk::Offset3D { x: (u.x >> level) as i32, y: (u.y >> level) as i32, z: 0 })
                                .image_extent(vk::Extent3D { width: s, height: s, depth: 1 }),
                        );
                        off += px.len();
                        if s > 1 {
                            s /= 2;
                        }
                    }
                }
                let range = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: MIP_LEVELS, base_array_layer: 0, layer_count: 1 };
                let barrier = |old, new, sa, da| {
                    [vk::ImageMemoryBarrier::default()
                        .old_layout(old)
                        .new_layout(new)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(self.atlas.0)
                        .subresource_range(range)
                        .src_access_mask(sa)
                        .dst_access_mask(da)]
                };
                dev.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &barrier(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::AccessFlags::SHADER_READ, vk::AccessFlags::TRANSFER_WRITE),
                );
                dev.cmd_copy_buffer_to_image(cmd, stage, self.atlas.0, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions);
                dev.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &barrier(vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::SHADER_READ),
                );
            }
            let fc = frame.globals.fog_color;
            let clears = [
                vk::ClearValue { color: vk::ClearColorValue { float32: [fc[0], fc[1], fc[2], 1.0] } },
                vk::ClearValue { depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0 } },
            ];
            let area = vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: self.extent };
            dev.cmd_begin_render_pass(
                cmd,
                &vk::RenderPassBeginInfo::default().render_pass(self.render_pass).framebuffer(self.framebuffers[image_index as usize]).render_area(area).clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            dev.cmd_set_viewport(cmd, 0, &[vk::Viewport { x: 0.0, y: 0.0, width: self.extent.width as f32, height: self.extent.height as f32, min_depth: 0.0, max_depth: 1.0 }]);
            dev.cmd_set_scissor(cmd, 0, &[area]);
            dev.cmd_set_line_width(cmd, 1.0);
            dev.cmd_bind_descriptor_sets(cmd, vk::PipelineBindPoint::GRAPHICS, self.layout, 0, &[set], &[]);
            let stages = vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT;
            let push = |offset: [f32; 4], color: [f32; 4]| {
                let d = [offset, color];
                dev.cmd_push_constants(cmd, self.layout, stages, 0, std::slice::from_raw_parts(d.as_ptr() as *const u8, 32));
            };
            push([0.0; 4], [0.0; 4]);

            dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_sky);
            dev.cmd_draw(cmd, 3, 1, 0, 0);

            dev.cmd_bind_index_buffer(cmd, self.index_buf.0, 0, vk::IndexType::UINT32);
            if fan_count > 0 {
                dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_sky_alpha);
                push([0.0; 4], [1.0; 4]);
                dev.cmd_bind_vertex_buffers(cmd, 0, &[dyn2], &[0]);
                dev.cmd_draw(cmd, fan_count, 1, 0, 0);
            }
            if frame.star_brightness > 0.0 && self.stars.2 > 0 {
                let b = frame.star_brightness;
                dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_sky_add);
                push([0.0, 0.0, 0.0, 2.0], [b, b, b, b]);
                dev.cmd_bind_vertex_buffers(cmd, 0, &[self.stars.0], &[0]);
                dev.cmd_draw_indexed(cmd, self.stars.2 * 6, 1, 0, 0, 0);
            }
            // Fancy clouds: depth first, then colour, so only the nearest
            // surface of the layer is blended.
            let draw_clouds = |c: &CloudDraw| {
                if let Some((buf, _, quads)) = self.cloud_mesh {
                    dev.cmd_bind_vertex_buffers(cmd, 0, &[buf], &[0]);
                    push([c.offset[0], c.offset[1], c.offset[2], 0.0], [c.color[0], c.color[1], c.color[2], 0.8]);
                    dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_cloud_depth);
                    dev.cmd_draw_indexed(cmd, quads * 6, 1, 0, 0, 0);
                    dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_cloud_color);
                    dev.cmd_draw_indexed(cmd, quads * 6, 1, 0, 0, 0);
                }
            };
            if let Some(c) = &frame.clouds {
                if c.before_terrain {
                    draw_clouds(c);
                }
            }
            dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_opaque);
            for (_, o, gm) in &visible {
                if gm.opaque_quads > 0 {
                    push([o[0], o[1], o[2], 0.0], [0.0; 4]);
                    dev.cmd_bind_vertex_buffers(cmd, 0, &[gm.buffer], &[0]);
                    dev.cmd_draw_indexed(cmd, gm.opaque_quads * 6, 1, 0, 0, 0);
                }
            }
            if outline_count > 0 {
                dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_line);
                if self.wide_lines {
                    dev.cmd_set_line_width(cmd, 2.0);
                }
                push([0.0; 4], [0.0, 0.0, 0.0, 0.4]);
                dev.cmd_bind_vertex_buffers(cmd, 0, &[dynbuf], &[0]);
                dev.cmd_draw(cmd, outline_count, 1, 0, 0);
            }
            dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_translucent);
            for (_, o, gm) in visible.iter().rev() {
                if gm.translucent_quads > 0 {
                    push([o[0], o[1], o[2], 0.0], [0.0; 4]);
                    dev.cmd_bind_vertex_buffers(cmd, 0, &[gm.buffer], &[0]);
                    dev.cmd_draw_indexed(cmd, gm.translucent_quads * 6, 1, 0, (gm.opaque_quads * 4) as i32, 0);
                }
            }
            if let Some(c) = &frame.clouds {
                if !c.before_terrain {
                    draw_clouds(c);
                }
            }
            dev.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipe_ui);
            push([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
            dev.cmd_bind_vertex_buffers(cmd, 0, &[dynbuf], &[0]);
            dev.cmd_draw(cmd, cross_count, 1, cross_first, 0);
            dev.cmd_end_render_pass(cmd);

            // Optional screenshot readback.
            let mut shot: Option<(vk::Buffer, vk::DeviceMemory)> = None;
            if frame.screenshot.is_some() {
                let size = self.extent.width as u64 * self.extent.height as u64 * 4;
                let host = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
                let (b, mem) = self.create_buffer(size, vk::BufferUsageFlags::TRANSFER_DST, host);
                let img = self.images[image_index as usize];
                let range = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
                let bar = |old, new, sa, da| {
                    [vk::ImageMemoryBarrier::default()
                        .old_layout(old)
                        .new_layout(new)
                        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                        .image(img)
                        .subresource_range(range)
                        .src_access_mask(sa)
                        .dst_access_mask(da)]
                };
                dev.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &bar(vk::ImageLayout::PRESENT_SRC_KHR, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::AccessFlags::COLOR_ATTACHMENT_WRITE, vk::AccessFlags::TRANSFER_READ),
                );
                let region = [vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 })
                    .image_extent(vk::Extent3D { width: self.extent.width, height: self.extent.height, depth: 1 })];
                dev.cmd_copy_image_to_buffer(cmd, img, vk::ImageLayout::TRANSFER_SRC_OPTIMAL, b, &region);
                dev.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &bar(vk::ImageLayout::TRANSFER_SRC_OPTIMAL, vk::ImageLayout::PRESENT_SRC_KHR, vk::AccessFlags::TRANSFER_READ, vk::AccessFlags::empty()),
                );
                shot = Some((b, mem));
            }
            dev.end_command_buffer(cmd).unwrap();

            let wait = [image_available];
            let signal = [render_finished];
            let cmds = [cmd];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            dev.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().wait_semaphores(&wait).wait_dst_stage_mask(&wait_stages).command_buffers(&cmds).signal_semaphores(&signal)],
                fence,
            )
            .unwrap();

            if let (Some((b, mem)), Some(path)) = (shot, frame.screenshot.as_ref()) {
                dev.wait_for_fences(&[fence], true, u64::MAX).unwrap();
                let (w, h) = (self.extent.width, self.extent.height);
                let size = (w * h * 4) as usize;
                let p = dev.map_memory(mem, 0, size as u64, vk::MemoryMapFlags::empty()).unwrap() as *const u8;
                let src = std::slice::from_raw_parts(p, size);
                let mut rgb = Vec::with_capacity((w * h * 3) as usize);
                let bgr = self.format.format == vk::Format::B8G8R8A8_UNORM || self.format.format == vk::Format::B8G8R8A8_SRGB;
                for px in src.chunks(4) {
                    if bgr {
                        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
                    } else {
                        rgb.extend_from_slice(&[px[0], px[1], px[2]]);
                    }
                }
                dev.unmap_memory(mem);
                dev.destroy_buffer(b, None);
                dev.free_memory(mem, None);
                if let Ok(file) = std::fs::File::create(path) {
                    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
                    enc.set_color(png::ColorType::Rgb);
                    enc.set_depth(png::BitDepth::Eight);
                    if let Ok(mut wr) = enc.write_header() {
                        let _ = wr.write_image_data(&rgb);
                    }
                }
            }

            let swapchains = [self.swapchain];
            let indices = [image_index];
            match self.swap_loader.queue_present(self.queue, &vk::PresentInfoKHR::default().wait_semaphores(&signal).swapchains(&swapchains).image_indices(&indices)) {
                Ok(sub) => {
                    if sub {
                        self.needs_recreate = true;
                    }
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
                Err(e) => panic!("queue_present: {e}"),
            }
            true
        }
    }
}

fn find_mem_type(props: &vk::PhysicalDeviceMemoryProperties, bits: u32, flags: vk::MemoryPropertyFlags) -> u32 {
    for i in 0..props.memory_type_count {
        if bits & (1 << i) != 0 && props.memory_types[i as usize].property_flags.contains(flags) {
            return i;
        }
    }
    panic!("no suitable Vulkan memory type");
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for (_, b, m) in self.garbage.drain(..) {
                self.device.destroy_buffer(b, None);
                self.device.free_memory(m, None);
            }
            for (_, m) in self.meshes.drain() {
                if m.buffer != vk::Buffer::null() {
                    self.device.destroy_buffer(m.buffer, None);
                    self.device.free_memory(m.memory, None);
                }
            }
            for f in self.frames.drain(..) {
                self.device.destroy_semaphore(f.image_available, None);
                self.device.destroy_semaphore(f.render_finished, None);
                self.device.destroy_fence(f.fence, None);
                self.device.destroy_buffer(f.ubo, None);
                self.device.free_memory(f.ubo_mem, None);
                self.device.destroy_buffer(f.dynbuf, None);
                self.device.free_memory(f.dyn_mem, None);
                self.device.destroy_buffer(f.dyn2, None);
                self.device.free_memory(f.dyn2_mem, None);
                self.device.destroy_buffer(f.stage, None);
                self.device.free_memory(f.stage_mem, None);
            }
            self.destroy_swapchain_views();
            self.device.destroy_buffer(self.index_buf.0, None);
            self.device.free_memory(self.index_buf.1, None);
            self.device.destroy_image_view(self.atlas.2, None);
            self.device.destroy_image(self.atlas.0, None);
            self.device.free_memory(self.atlas.1, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_buffer(self.stars.0, None);
            self.device.free_memory(self.stars.1, None);
            if let Some((b, m, _)) = self.cloud_mesh.take() {
                self.device.destroy_buffer(b, None);
                self.device.free_memory(m, None);
            }
            for p in [self.pipe_sky, self.pipe_opaque, self.pipe_translucent, self.pipe_line, self.pipe_ui, self.pipe_sky_alpha, self.pipe_sky_add, self.pipe_cloud_depth, self.pipe_cloud_color] {
                self.device.destroy_pipeline(p, None);
            }
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_descriptor_set_layout(self.set_layout, None);
            self.device.destroy_descriptor_pool(self.desc_pool, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_command_pool(self.pool, None);
            if self.swapchain != vk::SwapchainKHR::null() {
                self.swap_loader.destroy_swapchain(self.swapchain, None);
            }
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            if let Some((loader, m)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(m, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}
