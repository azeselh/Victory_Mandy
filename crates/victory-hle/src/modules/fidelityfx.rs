//! `amd_fidelityfx_loader_xbox.dll`: AMD FidelityFX's API, served by the
//! host's PC build of it on the host GPU when the user names a folder that
//! holds one (`VICTORY_FIDELITYFX_DIR`) and selects a 4K FSR profile.
//! Other profiles keep the title's own console DLLs.
//!
//! Both builds have the same API (the FidelityFX SDK's, MIT): five functions
//! over chains of descriptions, each starting with a type id and the next
//! one's address; contexts are opaque pointers. The console-specific parts
//! are translated: the backend description names the console device (type
//! 0x0100_0002; on PC type 2 with the host device), and a dispatch names the
//! title's command list and resources (their host list and host copies, in
//! the states the title gives, see `victory_gpu::external`). The rest passes
//! through unchanged: sizes, flags, jitter, motion-vector scale, camera
//! values, the resource descriptions, the context. Only the upscaler: other
//! effects (frame generation) are refused.
#![allow(non_snake_case)]
// Title code calls these through its import table.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock};

use victory_gpu::object::Object;
use windows::Win32::Graphics::Direct3D12::ID3D12Device;
use windows::core::Interface;

use crate::{HleModule, hle_export};

/// The PC build's upscaler, which has the five functions itself.
const DLL: &str = "amd_fidelityfx_upscaler_dx12.dll";

// Return codes.
const OK: u32 = 0;
const UNKNOWN_TYPE: u32 = 2;
const NO_PROVIDER: u32 = 4;
const PARAMETER: u32 = 6;

// Description types.
const CREATE_UPSCALE: u64 = 0x1_0000;
const DISPATCH_UPSCALE: u64 = 0x1_0001;
const CONSOLE_BACKEND: u64 = 0x100_0002;
const HOST_BACKEND: u64 = 2;
const GET_VERSIONS: u64 = 4;
const OVERRIDE_VERSION: u64 = 5;
const GET_PROVIDER_VERSION: u64 = 6;
const MEMORY_USAGE_WITH_DEVICE: u64 = 0x1_0009;
const GET_RENDER_RESOLUTION: u64 = 0x1_0003;
const DISPLAY_RESOLUTION_MOTION_VECTORS: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    kind: u64,
    next: *mut Header,
}

/// An upscaler context: flags, largest render and output sizes, a message
/// callback.
#[repr(C)]
#[derive(Clone, Copy)]
struct CreateUpscale {
    header: Header,
    flags: u32,
    max_render: [u32; 2],
    max_output: [u32; 2],
    message: usize,
}

/// The backend description: a device.
#[repr(C)]
struct Backend {
    header: Header,
    device: *mut c_void,
}

/// A resource, its description (type, format, sizes, mips, flags, usage)
/// and the state it is in.
#[repr(C)]
#[derive(Clone, Copy)]
struct Resource {
    resource: *mut c_void,
    description: [u32; 8],
    state: u32,
}

/// One upscaling: the command list, then colour, depth, motion vectors,
/// exposure, reactive mask, transparency mask and output; then jitter,
/// motion-vector scale, sizes, sharpening, frame time, pre-exposure, reset,
/// camera and flags, which pass through.
#[repr(C)]
#[derive(Clone, Copy)]
struct DispatchUpscale {
    header: Header,
    list: *mut c_void,
    resources: [Resource; 7],
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    render_size: [u32; 2],
    upscale_size: [u32; 2],
    settings: [u32; 10],
}

const _: () = assert!(size_of::<CreateUpscale>() == 0x30);
const _: () = assert!(size_of::<Resource>() == 0x30);
const _: () = assert!(size_of::<DispatchUpscale>() == 0x1b0);

/// The versions a backend offers: count in and out, ids, names.
#[repr(C)]
#[derive(Clone, Copy)]
struct GetVersions {
    header: Header,
    create_kind: u64,
    device: *mut c_void,
    count: *mut u64,
    ids: *mut u64,
    names: *mut *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryUsageWithDevice {
    header: Header,
    device: *mut c_void,
    max_render: [u32; 2],
    max_output: [u32; 2],
    flags: u32,
    usage: *mut c_void,
}

#[repr(C)]
struct ProviderVersion {
    header: Header,
    id: u64,
    name: *const c_char,
}

#[repr(C)]
struct OverrideVersion {
    header: Header,
    id: u64,
}

type Create = unsafe extern "C" fn(*mut *mut c_void, *mut Header, *const c_void) -> u32;
type Destroy = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> u32;
/// Configure, query and dispatch.
type Call = unsafe extern "C" fn(*mut *mut c_void, *mut Header) -> u32;

struct Host {
    _library: Library,
    create: Create,
    destroy: Destroy,
    configure: Call,
    query: Call,
    dispatch: Call,
    device: ID3D12Device,
    provider_id: u64,
    /// The provider's version as the SDK names it (`4.1.2`, `3.1.5`).
    provider_name: String,
}

struct Library(windows_sys::Win32::Foundation::HMODULE);

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: this owns the reference acquired by LoadLibraryExW.
        unsafe {
            windows_sys::Win32::Foundation::FreeLibrary(self.0);
        }
    }
}

// SAFETY: D3D12 devices are free-threaded; the rest is code.
unsafe impl Send for Host {}
// SAFETY: as above.
unsafe impl Sync for Host {}

static HOST: OnceLock<Host> = OnceLock::new();
struct Context {
    // Serializes recording and destruction for this particular temporal history.
    live: Mutex<bool>,
    first_dispatch: std::sync::Once,
    // The FFX API requires pointers in the creation descriptor to remain
    // live through destroy, including our replacement backend.
    creation: CreateChain,
}

struct CreateChain {
    description: Box<CreateUpscale>,
    _backend: Box<Backend>,
    _version: Box<OverrideVersion>,
}

// SAFETY: after Create, the boxed descriptors are retained immutably.
// The device is held by Host; the remaining extension pointers belong
// to the title under the FFX context lifetime contract.
unsafe impl Send for CreateChain {}
// SAFETY: as above, no thread mutates the retained descriptors.
unsafe impl Sync for CreateChain {}

static CONTEXTS: Mutex<BTreeMap<usize, Arc<Context>>> = Mutex::new(BTreeMap::new());

fn fits(extent: [u32; 2], maximum: [u32; 2]) -> bool {
    extent[0] != 0 && extent[1] != 0 && extent[0] <= maximum[0] && extent[1] <= maximum[1]
}

fn dispatch_output(
    render: [u32; 2],
    output: [u32; 2],
    max_render: [u32; 2],
    max_output: [u32; 2],
) -> Option<[u32; 2]> {
    let output = if output == [0, 0] { max_output } else { output };
    (fits(render, max_render) && fits(output, max_output) && fits(render, output)).then_some(output)
}

fn minimum_extent(index: usize, flags: u32, render: [u32; 2], output: [u32; 2]) -> [u32; 2] {
    match index {
        6 => output,
        2 if flags & DISPLAY_RESOLUTION_MOTION_VECTORS != 0 => output,
        3 => [1, 1], // Optional exposure texture.
        _ => render,
    }
}

macro_rules! once {
    ($level:ident, $($arg:tt)*) => {{
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| tracing::$level!($($arg)*));
    }};
}

/// Select the PC provider before the title's import registry is initialized.
pub fn use_host_dir(dir: &Path) -> Result<(), String> {
    let (host, version) = load(dir)?;
    HOST.set(host)
        .map_err(|_| String::from("FidelityFX provider already initialized"))?;
    if !is_fsr_4_1_2(&version) {
        tracing::warn!(
            version,
            "FidelityFX: the SDK offers this adapter no FSR 4.1.2; FSR 3.1 is used in its place (its picture differs from 4's)"
        );
    }
    victory_gpu::streamline::set_fsr_upscaler(version.clone());
    tracing::info!(folder = %dir.display(), version, "FidelityFX: selected PC upscaler");
    Ok(())
}

/// Active PC FidelityFX upscaler version string (`"4.1.2"`, `"3.1.5"`), if loaded.
pub fn version() -> Option<&'static str> {
    HOST.get().map(|h| h.provider_name.as_str())
}

/// Whether the host's FidelityFX serves the title's.
pub fn serving() -> bool {
    HOST.get().is_some()
}

fn load(dir: &Path) -> Result<(Host, String), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
        LoadLibraryExW,
    };
    // The search flags below want a full path.
    let path = std::path::absolute(dir.join(DLL)).map_err(|e| e.to_string())?;
    if !path.is_file() {
        return Err(format!("no {DLL} there"));
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // Its own folder and System32 only, never the current folder.
    let search = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
    // SAFETY: a NUL-terminated path to the user's FidelityFX build.
    let module = unsafe { LoadLibraryExW(wide.as_ptr(), null_mut(), search) };
    if module.is_null() {
        return Err(format!(
            "{DLL} did not load ({})",
            std::io::Error::last_os_error()
        ));
    }
    let library = Library(module);
    let find = |name: &CStr| {
        // SAFETY: a loaded module and a NUL-terminated name.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
            .map(|f| f as *const c_void)
            .ok_or_else(|| format!("{DLL} has no {}", name.to_string_lossy()))
    };
    let device = victory_gpu::external::host_device().ok_or("there is no host GPU")?;
    // SAFETY: the SDK's ffxQuery export, with its declared signature.
    let query = unsafe { std::mem::transmute::<*const c_void, Call>(find(c"ffxQuery")?) };
    let (provider_id, version) = select_version(query, &device)?;
    // SAFETY: the API's exports, with its signatures.
    let host = unsafe {
        Host {
            create: std::mem::transmute::<*const c_void, Create>(find(c"ffxCreateContext")?),
            destroy: std::mem::transmute::<*const c_void, Destroy>(find(c"ffxDestroyContext")?),
            configure: std::mem::transmute::<*const c_void, Call>(find(c"ffxConfigure")?),
            query,
            dispatch: std::mem::transmute::<*const c_void, Call>(find(c"ffxDispatch")?),
            _library: library,
            device,
            provider_id,
            provider_name: version.clone(),
        }
    };
    Ok((host, version))
}

fn is_fsr_4_1_2(name: &str) -> bool {
    name.strip_prefix("4.1.2")
        .is_some_and(|suffix| !suffix.starts_with(|c: char| c.is_ascii_digit()))
}

/// The SDK supplies the version id to put in the context creation chain.
fn select_version(query_fn: Call, device: &ID3D12Device) -> Result<(u64, String), String> {
    let mut count = 0u64;
    let mut query = GetVersions {
        header: Header {
            kind: GET_VERSIONS,
            next: null_mut(),
        },
        create_kind: CREATE_UPSCALE,
        device: device.as_raw(),
        count: &mut count,
        ids: null_mut(),
        names: null_mut(),
    };
    // SAFETY: a version query naming the host device.
    let code = unsafe { query_fn(null_mut(), &mut query.header) };
    if code != OK || count == 0 {
        return Err(format!(
            "it offers no upscaler for the host GPU (code {code})"
        ));
    }
    let mut names = vec![null::<c_char>(); count as usize];
    let mut ids = vec![0u64; names.len()];
    query.names = names.as_mut_ptr();
    query.ids = ids.as_mut_ptr();
    // SAFETY: as above, with room for `count` names.
    let code = unsafe { query_fn(null_mut(), &mut query.header) };
    if code != OK {
        return Err(format!(
            "upscaler version names could not be queried (code {code})"
        ));
    }
    let mut offered = Vec::new();
    for (id, name) in ids.into_iter().zip(names).take(count as usize) {
        if !name.is_null() {
            // SAFETY: NUL-terminated name owned by the SDK.
            let name = unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned();
            offered.push((id, name));
        }
    }
    choose(offered)
}

/// The patch number of an FSR 3.1 provider's name (`3.1.5` → 5); `None` for
/// any other version (`3.10.0`, `2.3.4`, `4.1.2`).
fn fsr_3_1_patch(name: &str) -> Option<u32> {
    let patch = name.strip_prefix("3.1.")?;
    let digits = patch.len() - patch.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    patch[..digits].parse().ok()
}

/// The provider to ask for, of those the SDK offers for the adapter: FSR
/// 4.1.2 where there is one, otherwise the newest FSR 3.1 (which runs on any
/// D3D12 card; AMD offers 4 on few). With neither, what was offered is said.
fn choose(offered: Vec<(u64, String)>) -> Result<(u64, String), String> {
    if let Some(at) = offered.iter().position(|(_, name)| is_fsr_4_1_2(name)) {
        return Ok(offered.into_iter().nth(at).unwrap());
    }
    let newest = offered
        .iter()
        .enumerate()
        .filter_map(|(at, (_, name))| fsr_3_1_patch(name).map(|patch| (patch, at)))
        .max_by_key(|&(patch, _)| patch);
    match newest {
        Some((_, at)) => Ok(offered.into_iter().nth(at).unwrap()),
        None => {
            let names: Vec<&str> = offered.iter().map(|(_, name)| name.as_str()).collect();
            Err(format!(
                "the SDK offers this adapter neither FSR 4.1.2 nor FSR 3.1 (offered: {}); run without -Fsr4K (VICTORY_FSR_4K)",
                if names.is_empty() { "nothing".into() } else { names.join(", ") }
            ))
        }
    }
}

/// Puts the host backend after the title's upscaler description, in place
/// of the console's backend that follows it (where the title puts it);
/// anywhere else the console's is left for the host DLL to ignore, ours
/// coming first.
fn link(upscale: &mut CreateUpscale, backend: &mut Backend) {
    let after = upscale.header.next;
    // SAFETY: the title's chain.
    backend.header.next = match unsafe { after.as_ref() } {
        Some(h) if h.kind == CONSOLE_BACKEND => h.next,
        _ => after,
    };
    backend.header.kind = HOST_BACKEND;
    upscale.header.next = &raw mut backend.header;
}

/// A FidelityFX resource state as the host resource state it means (as the
/// PC backend reads it); `None` for generic read and indirect arguments,
/// which a host texture cannot be in.
fn host_state(state: u32) -> Option<u32> {
    Some(match state {
        0x1 | 0x80 => 0,  // common, present
        0x2 => 0x8,       // unordered access
        0x4 => 0x40,      // compute read
        0x8 => 0x80,      // pixel read
        0xc => 0xc0,      // both
        0x10 => 0x800,    // copy source
        0x20 => 0x400,    // copy destination
        0x100 => 0x4,     // render target
        0x200 => 0x10,    // depth write
        _ => return None, // generic read, indirect arguments
    })
}

extern "C" fn ffxCreateContext(
    context: *mut *mut c_void,
    desc: *mut Header,
    memory: *const c_void,
) -> u32 {
    let Some(host) = HOST.get() else {
        return NO_PROVIDER;
    };
    if context.is_null() || desc.is_null() {
        return PARAMETER;
    }
    // SAFETY: the title passes a chain of descriptions.
    let kind = unsafe { (*desc).kind };
    if kind != CREATE_UPSCALE {
        once!(
            warn,
            kind = format_args!("{kind:#x}"),
            "FidelityFX: only the upscaler runs on the host; another effect was refused"
        );
        return NO_PROVIDER;
    }
    // SAFETY: an upscaler description, by its type.
    let mut upscale = Box::new(unsafe { desc.cast::<CreateUpscale>().read() });
    let mut backend = Box::new(Backend {
        header: Header {
            kind: HOST_BACKEND,
            next: null_mut(),
        },
        device: host.device.as_raw(),
    });
    link(&mut upscale, &mut backend);
    let mut requested = Box::new(OverrideVersion {
        header: Header {
            kind: OVERRIDE_VERSION,
            next: backend.header.next,
        },
        id: host.provider_id,
    });
    backend.header.next = &raw mut requested.header;
    // SAFETY: a PC chain; the title's later descriptions live for the call.
    let code = unsafe { (host.create)(context, &mut upscale.header, memory) };
    if code != OK {
        tracing::error!(code, render = ?upscale.max_render, output = ?upscale.max_output, flags = format_args!("{:#x}", upscale.flags), "FidelityFX: the host refused the title's upscaler");
        return code;
    }
    let mut version = ProviderVersion {
        header: Header {
            kind: GET_PROVIDER_VERSION,
            next: null_mut(),
        },
        id: 0,
        name: null(),
    };
    // SAFETY: a provider query on the context just made.
    let query_code = unsafe { (host.query)(context, &mut version.header) };
    // SAFETY: a NUL-terminated name the DLL keeps, if any.
    let name = (!version.name.is_null())
        .then(|| unsafe { CStr::from_ptr(version.name) }.to_string_lossy());
    if query_code != OK
        || version.id != host.provider_id
        || name.as_deref() != Some(host.provider_name.as_str())
    {
        tracing::warn!(
            query_code,
            provider = name.as_deref().unwrap_or("?"),
            requested = host.provider_name,
            "FidelityFX created a different provider than the one requested"
        );
        // SAFETY: the PC provider just created the context with this allocator.
        unsafe { (host.destroy)(context, memory) };
        return NO_PROVIDER;
    }
    // SAFETY: a successful SDK creation wrote an opaque context into this slot.
    tracing::info!(
        provider = name.as_deref().unwrap_or("?"),
        id = format_args!("{:#x}", version.id),
        render = ?upscale.max_render,
        output = ?upscale.max_output,
        flags = format_args!("{:#x}", upscale.flags),
        "FidelityFX: upscaler context on the host"
    );
    CONTEXTS.lock().unwrap_or_else(|e| e.into_inner()).insert(
        unsafe { *context } as usize,
        Arc::new(Context {
            live: Mutex::new(true),
            first_dispatch: std::sync::Once::new(),
            creation: CreateChain {
                description: upscale,
                _backend: backend,
                _version: requested,
            },
        }),
    );
    OK
}

extern "C" fn ffxDestroyContext(context: *mut *mut c_void, memory: *const c_void) -> u32 {
    let Some(host) = HOST.get() else {
        return NO_PROVIDER;
    };
    if context.is_null() {
        return PARAMETER;
    }
    // SAFETY: the title passes its context. Capture its value before destroy clears it.
    let handle = unsafe { *context } as usize;
    let ctx = CONTEXTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&handle)
        .cloned();
    let Some(ctx) = ctx else { return PARAMETER };
    let mut live = ctx.live.lock().unwrap_or_else(|e| e.into_inner());
    if !*live {
        return PARAMETER;
    }
    // The guest releases a context after submitting its frames. The host
    // may still be executing their lists, even after guest Execute returns.
    victory_gpu::external::finish_submitted();
    let code = unsafe { (host.destroy)(context, memory) };
    if code == OK {
        *live = false;
        let mut contexts = CONTEXTS.lock().unwrap_or_else(|e| e.into_inner());
        // A concurrent Create may already have reused the released SDK address.
        if contexts
            .get(&handle)
            .is_some_and(|current| Arc::ptr_eq(current, &ctx))
        {
            contexts.remove(&handle);
        }
    }
    code
}

/// Settings (key-value pairs, debug messages) name nothing console-specific.
extern "C" fn ffxConfigure(context: *mut *mut c_void, desc: *mut Header) -> u32 {
    // SAFETY: the title passes its context and descriptions.
    HOST.get()
        .map_or(NO_PROVIDER, |h| unsafe { (h.configure)(context, desc) })
}

extern "C" fn ffxQuery(context: *mut *mut c_void, desc: *mut Header) -> u32 {
    let Some(host) = HOST.get() else {
        return NO_PROVIDER;
    };
    if desc.is_null() {
        return PARAMETER;
    }
    // SAFETY: the title passes a description.
    let kind = unsafe { (*desc).kind };
    // Substitute the native device; the SDK returns values through the
    // title's original output pointers in both request structures.
    if kind == GET_VERSIONS {
        let mut query = unsafe { *desc.cast::<GetVersions>() };
        query.device = host.device.as_raw();
        return unsafe { (host.query)(context, &mut query.header) };
    }
    if kind == MEMORY_USAGE_WITH_DEVICE {
        let mut query = unsafe { *desc.cast::<MemoryUsageWithDevice>() };
        query.device = host.device.as_raw();
        return unsafe { (host.query)(context, &mut query.header) };
    }
    // SAFETY: as above.
    let code = unsafe { (host.query)(context, desc) };
    if kind == GET_RENDER_RESOLUTION && code != OK {
        tracing::warn!(
            code,
            "FidelityFX provider refused the render-resolution query"
        );
    }
    code
}

extern "C" fn ffxDispatch(context: *mut *mut c_void, desc: *mut Header) -> u32 {
    let Some(host) = HOST.get() else {
        return NO_PROVIDER;
    };
    if desc.is_null() {
        return PARAMETER;
    }
    // SAFETY: the title passes a description.
    let kind = unsafe { (*desc).kind };
    if kind != DISPATCH_UPSCALE {
        once!(
            warn,
            kind = format_args!("{kind:#x}"),
            "FidelityFX: only upscaling runs on the host; a dispatch was refused"
        );
        return UNKNOWN_TYPE;
    }
    // SAFETY: an upscaling, by its type.
    let dispatch = unsafe { desc.cast::<DispatchUpscale>().read() };
    if context.is_null() {
        once!(warn, "FidelityFX dispatch arrived without a context");
        return PARAMETER;
    }
    if let Some(index) = [0, 1, 2, 6]
        .into_iter()
        .find(|&i| dispatch.resources[i].resource.is_null())
    {
        once!(
            warn,
            index,
            "FidelityFX dispatch is missing a required texture"
        );
        return PARAMETER;
    }
    // SAFETY: the title passes the same opaque context slot as for creation.
    let handle = unsafe { *context } as usize;
    let ctx = CONTEXTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&handle)
        .cloned();
    let Some(ctx) = ctx else {
        once!(
            warn,
            "FidelityFX dispatch referenced an unregistered context"
        );
        return PARAMETER;
    };
    {
        let live = ctx.live.lock().unwrap_or_else(|e| e.into_inner());
        if !*live {
            return PARAMETER;
        }
    }
    let create = &ctx.creation.description;
    let Some(output_size) = dispatch_output(
        dispatch.render_size,
        dispatch.upscale_size,
        create.max_render,
        create.max_output,
    ) else {
        once!(warn, render = ?dispatch.render_size, output = ?dispatch.upscale_size,
            max_render = ?create.max_render, max_output = ?create.max_output,
            "FidelityFX rejected dispatch dimensions outside the context's allocations");
        return PARAMETER;
    };
    let list = dispatch.list.cast::<Object>().cast_const();
    let queued = QueuedDispatch { dispatch, handle, output_size, ctx };
    // Run inline on non-thin command lists, or queued in submission order on VICTORY_THIN=1.
    if victory_gpu::external::record(list, move |rec| {
        let mut q = queued;
        record_dispatch(rec, host, &mut q.dispatch, q.handle, &q.ctx, q.output_size)
    }) {
        OK
    } else {
        once!(
            warn,
            "FidelityFX could not record to the title command list"
        );
        PARAMETER
    }
}

struct QueuedDispatch {
    dispatch: DispatchUpscale,
    handle: usize,
    output_size: [u32; 2],
    ctx: Arc<Context>,
}

// SAFETY: `dispatch` carries title `Object*` handles that stay alive until the
// command list has been submitted, and `ctx` serializes execution via `live`.
unsafe impl Send for QueuedDispatch {}

fn build_upscale(
    dispatch: &DispatchUpscale,
    flags: u32,
    output_size: [u32; 2],
    context: usize,
) -> victory_gpu::streamline::Upscale {
    let settings = &dispatch.settings;
    let frame_ms = f32::from_bits(settings[2]);
    let pre_exposure = f32::from_bits(settings[3]);
    let reset = (settings[4] & 0xff) != 0;
    let camera_near = f32::from_bits(settings[5]);
    let camera_far = f32::from_bits(settings[6]);
    let fov_y = f32::from_bits(settings[7]);
    let resources = dispatch.resources.map(|r| (r.resource.cast::<Object>().cast_const(), r.state));
    victory_gpu::streamline::Upscale {
        context,
        list: dispatch.list.cast::<Object>().cast_const(),
        resources,
        camera: victory_gpu::streamline::plan::Camera {
            render: dispatch.render_size,
            output: output_size,
            jitter: dispatch.jitter,
            motion_scale: dispatch.motion_scale,
            near: camera_near,
            far: camera_far,
            fov_y,
            flags,
            reset,
        },
        pre_exposure,
        frame_ms,
    }
}

fn record_dispatch(
    list: &victory_gpu::external::Recording<'_>,
    host: &Host,
    dispatch: &mut DispatchUpscale,
    handle: usize,
    ctx: &Context,
    output_size: [u32; 2],
) -> bool {
    let live = ctx.live.lock().unwrap_or_else(|e| e.into_inner());
    if !*live {
        return false;
    }
    let output = dispatch.resources[6].resource.cast::<Object>();
    let upscale_meta = build_upscale(dispatch, ctx.creation.description.flags, output_size, handle);
    // Alive for the call (the list holds them too).
    let mut textures = Vec::new();
    let mut backing_extents = [None; 7];
    for (index, r) in dispatch.resources.iter_mut().enumerate() {
        if r.resource.is_null() {
            continue;
        }
        if index < 6 && list.prepare_input(r.resource.cast::<Object>()).is_none() {
            once!(
                warn,
                index,
                "FidelityFX input could not be prepared on the title command list"
            );
            return false;
        }
        let wanted = host_state(r.state);
        let texture = wanted.and_then(|w| {
            list.texture(r.resource.cast::<Object>(), w)
                .filter(|&(_, state)| state == w)
        });
        let Some((texture, _)) = texture else {
            once!(
                warn,
                index,
                state = r.state,
                "FidelityFX: a resource has no host texture in its state; the upscaling did not run"
            );
            return false;
        };
        let desc = unsafe { texture.GetDesc() };
        backing_extents[index] = Some((desc.Width, desc.Height));
        let needed = minimum_extent(
            index,
            ctx.creation.description.flags,
            dispatch.render_size,
            output_size,
        );
        if desc.Width < u64::from(needed[0]) || desc.Height < needed[1] {
            once!(
                warn,
                index,
                ?needed,
                width = desc.Width,
                height = desc.Height,
                "FidelityFX resource cannot cover the active dispatch rectangle"
            );
            return false;
        }
        r.resource = texture.as_raw();
        textures.push(texture);
    }
    once!(info, formats = ?textures.iter().map(|t| {
        // SAFETY: a live texture.
        unsafe { t.GetDesc() }.Format.0
    }).collect::<Vec<_>>(), "FidelityFX: first upscaling, host formats of its textures");
    dispatch.list = list.commands().as_raw();
    let mut raw_ctx = handle as *mut c_void;
    // SAFETY: the PC dispatch on host objects. The context is live and locked.
    let code = unsafe { (host.dispatch)(&mut raw_ctx, &raw mut dispatch.header) };
    if code == OK {
        victory_gpu::external::output_written(output);
        list.note_upscale(&upscale_meta);
        ctx.first_dispatch.call_once(|| {
            tracing::info!(render = ?dispatch.render_size, output = ?output_size, ?backing_extents,
                jitter = ?dispatch.jitter, motion_vector_scale = ?dispatch.motion_scale,
                provider = %host.provider_name,
                "FidelityFX recorded the title's temporal dispatch");
        });
        true
    } else {
        once!(
            warn,
            code,
            "FidelityFX provider rejected the title dispatch"
        );
        false
    }
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "fidelityfx",
        dlls: &["amd_fidelityfx_loader_xbox.dll"],
        exports: vec![
            hle_export!(ffxConfigure),
            hle_export!(ffxCreateContext),
            hle_export!(ffxDestroyContext),
            hle_export!(ffxDispatch),
            hle_export!(ffxQuery),
        ],
    }
}

pub use interpose::Interpose;

/// Streamline interposer over the title's own `amd_fidelityfx_loader_xbox.dll`
/// imports when the PC FidelityFX upscaler (`serving()`) is not active.
mod interpose {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    use victory_gpu::object::Object;
    use victory_gpu::streamline::{self, Taken, Upscale, plan::Camera};
    use victory_loader::{ImportResolver, Provenance, Resolution};
    use victory_pe::ImportSymbol;

    pub const LOADER_DLL: &str = "amd_fidelityfx_loader_xbox.dll";

    const OK: u32 = 0;
    const PARAMETER: u32 = 6;
    const CREATE_UPSCALE: u64 = 0x1_0000;
    const DISPATCH_UPSCALE: u64 = 0x1_0001;

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    struct Header {
        kind: u64,
        next: *mut Header,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    struct CreateUpscale {
        header: Header,
        flags: u32,
        max_render: [u32; 2],
        max_output: [u32; 2],
        message: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    struct Resource {
        resource: *mut c_void,
        description: [u32; 8],
        state: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    struct DispatchUpscale {
        header: Header,
        list: *mut c_void,
        resources: [Resource; 7],
        jitter: [f32; 2],
        motion_scale: [f32; 2],
        render_size: [u32; 2],
        upscale_size: [u32; 2],
        enable_sharpening: u8,
        sharpness: f32,
        frame_time_delta: f32,
        pre_exposure: f32,
        reset: u8,
        camera_near: f32,
        camera_far: f32,
        camera_fov_angle_vertical: f32,
        view_space_to_meters_factor: f32,
        flags: u32,
    }

    type Create = unsafe extern "C" fn(*mut *mut c_void, *mut Header, *const c_void) -> u32;
    type Destroy = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> u32;
    type Call = unsafe extern "C" fn(*mut *mut c_void, *mut Header) -> u32;

    static ORIGINALS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
    const NAMES: [&str; 5] = [
        "ffxCreateContext",
        "ffxDestroyContext",
        "ffxConfigure",
        "ffxQuery",
        "ffxDispatch",
    ];

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Created {
        flags: u32,
        max_render: [u32; 2],
        max_output: [u32; 2],
        clamped: bool,
    }

    static CONTEXTS: Mutex<Option<HashMap<usize, Created>>> = Mutex::new(None);

    macro_rules! once {
        ($level:ident, $($arg:tt)*) => {{
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(|| tracing::$level!($($arg)*));
        }};
    }

    /// Binds the title's imports of [`LOADER_DLL`] to the functions here when
    /// Streamline (or FSR 3 frame generation without the PC upscaler) is on,
    /// keeping the title's own DLL for fallback.
    pub struct Interpose<'a> {
        pub inner: &'a dyn ImportResolver,
    }

    impl ImportResolver for Interpose<'_> {
        fn resolve(&self, dll: &str, symbol: &ImportSymbol) -> Resolution {
            let found = self.inner.resolve(dll, symbol);
            if super::serving()
                || !dll.eq_ignore_ascii_case(LOADER_DLL)
                || !streamline::serving()
            {
                return found;
            }
            match (&found, symbol) {
                (&Resolution::Found { address, .. }, ImportSymbol::Name { name, .. }) => {
                    match interpose(name, address) {
                        Some(ours) => Resolution::Found {
                            address: ours,
                            provenance: Provenance::Hle,
                        },
                        None => found,
                    }
                }
                _ => found,
            }
        }
    }

    fn interpose(name: &str, original: usize) -> Option<usize> {
        let i = NAMES.iter().position(|n| *n == name)?;
        ORIGINALS[i].store(original, Relaxed);
        let ours = [
            ffxCreateContext as *const () as usize,
            ffxDestroyContext as *const () as usize,
            ffxConfigure as *const () as usize,
            ffxQuery as *const () as usize,
            ffxDispatch as *const () as usize,
        ][i];
        tracing::info!(
            name,
            original = format_args!("{original:#x}"),
            "FidelityFX: the title's import goes through ours (frame generation / streamline)"
        );
        Some(ours)
    }

    fn original(i: usize) -> Option<*const ()> {
        let address = ORIGINALS[i].load(Relaxed);
        (address != 0).then_some(address as *const ())
    }

    fn contexts<T>(f: impl FnOnce(&mut HashMap<usize, Created>) -> T) -> T {
        let mut contexts = CONTEXTS.lock().unwrap_or_else(|e| e.into_inner());
        f(contexts.get_or_insert_with(HashMap::new))
    }

    const SHIPPING_MAX_OUTPUT: [u32; 2] = [1920, 1080];
    const SHIPPING_MAX_RENDER: [u32; 2] = [1280, 720];

    fn shipping_sizes(render: [u32; 2], output: [u32; 2]) -> Option<([u32; 2], [u32; 2])> {
        ((render, output) != (SHIPPING_MAX_RENDER, SHIPPING_MAX_OUTPUT))
            .then_some((SHIPPING_MAX_RENDER, SHIPPING_MAX_OUTPUT))
    }

    extern "C" fn ffxCreateContext(
        context: *mut *mut c_void,
        desc: *mut Header,
        memory: *const c_void,
    ) -> u32 {
        let Some(create) = original(0) else { return PARAMETER };
        // SAFETY: the title's own function, with its arguments.
        let call = |desc: *mut Header| unsafe {
            std::mem::transmute::<*const (), Create>(create)(context, desc, memory)
        };
        // SAFETY: the title passes a chain of descriptions.
        let upscaler = !desc.is_null() && unsafe { (*desc).kind } == CREATE_UPSCALE;
        if !upscaler {
            return call(desc);
        }
        // SAFETY: an upscaler description, by its type.
        let asked = unsafe { desc.cast::<CreateUpscale>().read() };
        let shipping = streamline::upscaling()
            .then(|| shipping_sizes(asked.max_render, asked.max_output))
            .flatten();
        let clamped = shipping.is_some();
        let code = if let Some((render, output)) = shipping {
            let mut fitted = asked;
            fitted.max_render = render;
            fitted.max_output = output;
            // SAFETY: the title's description, rewritten for the call and put back.
            unsafe { desc.cast::<CreateUpscale>().write(fitted) };
            let code = call(desc);
            // SAFETY: as above.
            unsafe { desc.cast::<CreateUpscale>().write(asked) };
            code
        } else {
            call(desc)
        };
        if code != OK || context.is_null() {
            return code;
        }
        // SAFETY: the context was made.
        let handle = unsafe { *context } as usize;
        let created = Created {
            flags: asked.flags,
            max_render: asked.max_render,
            max_output: asked.max_output,
            clamped,
        };
        contexts(|c| c.insert(handle, created));
        code
    }

    extern "C" fn ffxDestroyContext(context: *mut *mut c_void, memory: *const c_void) -> u32 {
        let Some(destroy) = original(1) else { return PARAMETER };
        if !context.is_null() {
            // SAFETY: the title passes its context.
            let handle = unsafe { *context } as usize;
            if contexts(|c| c.remove(&handle)).is_some() {
                streamline::upscaler_destroyed(handle);
            }
        }
        // SAFETY: the title's own function, with its arguments.
        unsafe { std::mem::transmute::<*const (), Destroy>(destroy)(context, memory) }
    }

    extern "C" fn ffxConfigure(context: *mut *mut c_void, desc: *mut Header) -> u32 {
        let Some(configure) = original(2) else { return PARAMETER };
        // SAFETY: the title's own function, with its arguments.
        unsafe { std::mem::transmute::<*const (), Call>(configure)(context, desc) }
    }

    extern "C" fn ffxQuery(context: *mut *mut c_void, desc: *mut Header) -> u32 {
        let Some(query) = original(3) else { return PARAMETER };
        // SAFETY: the title's own function, with its arguments.
        unsafe { std::mem::transmute::<*const (), Call>(query)(context, desc) }
    }

    extern "C" fn ffxDispatch(context: *mut *mut c_void, desc: *mut Header) -> u32 {
        let Some(dispatch) = original(4) else { return PARAMETER };
        // SAFETY: the title's own function, with its arguments.
        let own = || unsafe { std::mem::transmute::<*const (), Call>(dispatch)(context, desc) };
        if context.is_null() || desc.is_null() {
            return own();
        }
        // SAFETY: the title passes a description and its context.
        let (kind, handle) = unsafe { ((*desc).kind, *context as usize) };
        if kind != DISPATCH_UPSCALE {
            return own();
        }
        let Some(created) = contexts(|c| c.get(&handle).copied()) else {
            return own();
        };
        // SAFETY: an upscaling, by its type.
        let d = unsafe { desc.cast::<DispatchUpscale>().read() };
        let resources = d.resources.map(|r| (r.resource.cast::<Object>().cast_const(), r.state));
        let up = Upscale {
            context: handle,
            list: d.list.cast::<Object>().cast_const(),
            resources,
            camera: Camera {
                render: d.render_size,
                output: if d.upscale_size == [0, 0] {
                    created.max_output
                } else {
                    d.upscale_size
                },
                jitter: d.jitter,
                motion_scale: d.motion_scale,
                near: d.camera_near,
                far: d.camera_far,
                fov_y: d.camera_fov_angle_vertical,
                flags: created.flags,
                reset: d.reset != 0,
            },
            pre_exposure: d.pre_exposure,
            frame_ms: d.frame_time_delta,
        };
        match streamline::upscale(&up) {
            Taken::Dlss => OK,
            Taken::Noted => own(),
            Taken::No(why) if created.clamped => {
                refused(why);
                once!(warn, "FidelityFX: upscaling refused on a clamped context");
                OK
            }
            Taken::No(why) => {
                streamline::forwarded();
                refused(why);
                own()
            }
        }
    }

    fn refused(why: &'static str) {
        static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&why) {
            seen.push(why);
            tracing::warn!("FidelityFX: the title's own upscaling runs: {why}");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn contexts_are_made_at_the_shipping_sizes() {
            let shipping = Some(([1280, 720], [1920, 1080]));
            assert_eq!(shipping_sizes([1706, 960], [2560, 1440]), shipping);
            assert_eq!(shipping_sizes([1280, 720], [1920, 1080]), None);
        }

        struct Fixed;
        impl ImportResolver for Fixed {
            fn resolve(&self, dll: &str, _: &ImportSymbol) -> Resolution {
                if dll.eq_ignore_ascii_case(LOADER_DLL) {
                    Resolution::Found {
                        address: 0x1234,
                        provenance: Provenance::Title,
                    }
                } else {
                    Resolution::Missing
                }
            }
        }

        #[test]
        fn without_streamline_the_titles_own_binds() {
            let name = |n: &str| ImportSymbol::Name {
                hint: 0,
                name: n.into(),
            };
            let interposed = Interpose { inner: &Fixed };
            assert_eq!(
                interposed.resolve("AMD_FIDELITYFX_LOADER_XBOX.DLL", &name("ffxDispatch")),
                Resolution::Found {
                    address: 0x1234,
                    provenance: Provenance::Title
                }
            );
            assert_eq!(
                interposed.resolve("other.dll", &name("ffxDispatch")),
                Resolution::Missing
            );
            assert_eq!(interpose("ffxGetVersion", 1), None);
            assert!(interpose("ffxQuery", 0x5678).is_some());
            assert_eq!(original(3), Some(0x5678 as *const ()));
            ORIGINALS[3].store(0, Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_logical_rectangles_against_the_current_context() {
        for display in [[1920, 1080], [2560, 1440], [3840, 2160]] {
            for divisor in [1.0_f32, 1.5, 1.7, 2.0, 3.0] {
                let render = display.map(|v| (v as f32 / divisor) as u32);
                assert_eq!(
                    dispatch_output(render, display, render, display),
                    Some(display)
                );
                assert_eq!(
                    dispatch_output(render, [0, 0], render, display),
                    Some(display)
                );
                assert_eq!(minimum_extent(2, 0x29, render, display), render);
                assert_eq!(minimum_extent(2, 0x2b, render, display), display);
                assert_eq!(minimum_extent(6, 0x29, render, display), display);
                assert_eq!(minimum_extent(3, 0x29, render, display), [1, 1]);
            }
        }
        assert_eq!(
            dispatch_output([0, 720], [0, 0], [1280, 720], [2560, 1440]),
            None
        );
        assert_eq!(
            dispatch_output([1281, 720], [0, 0], [1280, 720], [2560, 1440]),
            None
        );
        assert_eq!(
            dispatch_output([1280, 720], [0, 1440], [1280, 720], [2560, 1440]),
            None
        );
        assert_eq!(
            dispatch_output([1280, 720], [3000, 1440], [1280, 720], [2560, 1440]),
            None
        );
        assert_eq!(
            dispatch_output([1280, 720], [960, 540], [1280, 720], [2560, 1440]),
            None
        );
    }

    fn header(kind: u64, next: *mut Header) -> Header {
        Header { kind, next }
    }

    /// The title's chain: upscaler, console backend, version.
    #[test]
    fn the_console_backend_becomes_the_hosts() {
        let mut version = header(0x1_000b, null_mut());
        let mut console = Backend {
            header: header(CONSOLE_BACKEND, &raw mut version),
            device: 0x1234 as *mut c_void,
        };
        let mut upscale = CreateUpscale {
            header: header(CREATE_UPSCALE, &raw mut console.header),
            flags: 0x29,
            max_render: [1280, 720],
            max_output: [1920, 1080],
            message: 0,
        };
        let mut backend = Backend {
            header: header(0, null_mut()),
            device: 0x5678 as *mut c_void,
        };
        link(&mut upscale, &mut backend);
        assert_eq!(upscale.header.next, &raw mut backend.header);
        assert_eq!(backend.header.kind, HOST_BACKEND);
        assert_eq!(backend.header.next, &raw mut version);
        assert_eq!(backend.device, 0x5678 as *mut c_void);
        // Without a console backend right after it, ours goes in front.
        let mut upscale = CreateUpscale {
            header: header(CREATE_UPSCALE, &raw mut version),
            ..upscale
        };
        link(&mut upscale, &mut backend);
        assert_eq!(backend.header.next, &raw mut version);
    }

    /// Without the PC build the title's own DLL serves its imports.
    #[test]
    fn without_the_dll_the_titles_own_runs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).err().unwrap().contains(DLL));
        assert!(!serving());
        let symbol = victory_pe::ImportSymbol::Name {
            hint: 0,
            name: "ffxCreateContext".into(),
        };
        let resolver = crate::HleResolver::new(crate::HostPolicy::none());
        assert_eq!(
            resolver.classify("amd_fidelityfx_loader_xbox.dll", &symbol),
            None
        );
    }

    fn offered(names: &[&str]) -> Vec<(u64, String)> {
        names.iter().zip(1..).map(|(name, id)| (id, name.to_string())).collect()
    }

    /// FSR 4.1.2 where the SDK offers it; otherwise the newest 3.1, which
    /// is what a card without FSR 4 is offered; with neither, said so.
    #[test]
    fn the_provider_is_4_1_2_or_else_the_newest_3_1() {
        assert_eq!(choose(offered(&["3.1.5", "4.1.2", "2.3.4"])), Ok((2, "4.1.2".into())));
        assert_eq!(choose(offered(&["4.1.2 (ML)", "3.1.5"])), Ok((1, "4.1.2 (ML)".into())));
        assert_eq!(choose(offered(&["2.3.4", "3.1.4", "3.1.5"])), Ok((3, "3.1.5".into())));
        assert_eq!(choose(offered(&["3.1.10", "3.1.9"])), Ok((1, "3.1.10".into())));
        // Other versions are not taken for either.
        assert_eq!(choose(offered(&["4.1.20", "3.1.5", "3.10.0"])), Ok((2, "3.1.5".into())));
        let none = choose(offered(&["2.3.4", "4.0.3"])).unwrap_err();
        assert!(none.contains("2.3.4, 4.0.3") && none.contains("-Fsr4K"), "{none}");
        assert!(choose(Vec::new()).unwrap_err().contains("nothing"));
        assert_eq!(fsr_3_1_patch("3.1.5"), Some(5));
        assert_eq!(fsr_3_1_patch("3.1.5-beta"), Some(5));
        assert_eq!(fsr_3_1_patch("3.1."), None);
        assert_eq!(fsr_3_1_patch("3.10.0"), None);
        assert_eq!(fsr_3_1_patch("4.1.2"), None);
    }

    #[test]
    fn resource_states_read_as_the_pc_backend_reads_them() {
        assert_eq!(host_state(0xc), Some(0xc0));
        assert_eq!(host_state(0x2), Some(0x8));
        assert_eq!(host_state(0x200), Some(0x10));
        assert_eq!(host_state(0x14), None);
    }
}
