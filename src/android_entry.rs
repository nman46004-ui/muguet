// muguet-android - android_entry.rs
// Minimal Android entry point for Muguet using android-activity crate
// Runs via adb shell - no Java, no Gradle, no APK packaging required

use android_activity::{AndroidApp, InputStatus, MainEvent, MotionEventAction, KeyEventAction};
use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use wasmtime::*;
use wgpu::{Backends, InstanceDescriptor, RequestAdapterOptions, PowerPreference, TextureFormat};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle};
use ndk_glue::NativeWindow;
use jni::{JNIEnv, JavaVM};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jint, jlong, jfloat, jboolean, jstring};

mod audio;
mod config;
mod fs;
mod gpu;
mod host;
mod input;
mod import;
mod io;
mod mp;
mod pad;
#[cfg(feature = "jit")]
mod patch;
mod proto;
mod server;
mod touch_android;

use crate::{gpu, input, Runtime, config};

pub struct ImportSpec {
    pub module: String,
    pub name: String,
    pub ty: FuncType,
}

pub struct Runtime {
    pub engine: Engine,
    pub module: Module,
    pub mem: SharedMemory,
    pub imports: Vec<ImportSpec>,
    pub counts: Vec<AtomicU64>,
    pub start: Instant,
}

pub struct HostState {
    pub pthread: u64,
}

pub static RT: OnceLock<Arc<Runtime>> = OnceLock::new();
static JAVA_VM: OnceLock<JavaVM> = OnceLock::new();
static TOUCH_OVERLAY: OnceLock<JObject> = OnceLock::new();

// Launch config from Java
static LAUNCH_CONFIG: OnceLock<LaunchConfig> = OnceLock::new();

#[derive(Debug, Clone)]
struct LaunchConfig {
    start_mode: i32,
    scale: f32,
    fps_limit: i32,
    touch_sens: f32,
    fullscreen: bool,
    mute: bool,
    multiplayer: bool,
    host: bool,
    server_address: String,
    player_name: String,
}

#[no_mangle]
pub extern "system" fn Java_com_muguet_MainActivity_nativeSetLaunchConfig(
    mut env: JNIEnv,
    _class: JClass,
    start_mode: jint,
    scale: jfloat,
    fps_limit: jint,
    touch_sens: jfloat,
    fullscreen: jboolean,
    mute: jboolean,
    multiplayer: jboolean,
    host: jboolean,
    server_address: JString,
    player_name: JString,
) {
    let server = env.get_string(&server_address).map(|s| s.into()).unwrap_or_default();
    let name = env.get_string(&player_name).map(|s| s.into()).unwrap_or_default();
    
    let config = LaunchConfig {
        start_mode,
        scale,
        fps_limit,
        touch_sens,
        fullscreen: fullscreen != 0,
        mute: mute != 0,
        multiplayer: multiplayer != 0,
        host: host != 0,
        server_address: server,
        player_name: name,
    };
    
    LAUNCH_CONFIG.set(config).ok();
    eprintln!("[launch] config set: {:?}", LAUNCH_CONFIG.get());
}

fn engine_config() -> Config {
    let mut config = Config::new();
    config.wasm_memory64(true);
    config.wasm_threads(true);
    config.shared_memory(true);
    config.wasm_bulk_memory(true);
    config.wasm_multi_memory(true);
    #[cfg(feature = "jit")]
    config.cranelift_opt_level(OptLevel::Speed);
    config.memory_reservation(17 << 30);
    config.memory_guard_size(2 << 30);
    config.memory_may_move(false);
    #[cfg(feature = "jit")]
    unsafe {
        config.cranelift_flag_set("enable_heap_access_spectre_mitigation", "false");
        config.cranelift_flag_set("enable_table_access_spectre_mitigation", "false");
    }
    config
}

pub fn make_runtime(startup: &config::Startup) -> Result<Arc<Runtime>> {
    let engine = Engine::new(&engine_config()).map_err(anyhow::Error::from).context("engine")?;
    let t0 = Instant::now();
    let module = load_module(&engine, startup)?;
    eprintln!("[host] module ready in {:?}", t0.elapsed());

    let mut mem = None;
    let mut imports = Vec::new();
    for imp in module.imports() {
        match imp.ty() {
            ExternType::Memory(mt) => mem = Some(SharedMemory::new(&engine, mt)?),
            ExternType::Func(ty) => imports.push(ImportSpec {
                module: imp.module().to_string(),
                name: imp.name().to_string(),
                ty,
            }),
            other => anyhow::bail!("unsupported import {}.{}: {other:?}", imp.module(), imp.name()),
        }
    }
    let counts = imports.iter().map(|_| AtomicU64::new(0)).collect();
    let rt = Arc::new(Runtime {
        engine,
        module,
        mem: mem.context("module must import env.memory")?,
        imports,
        counts,
        start: Instant::now(),
    });
    RT.set(rt.clone()).ok();
    Ok(rt)
}

fn load_module(engine: &Engine, startup: &config::Startup) -> Result<Module> {
    let wasm_path = startup.wasm_path.clone();
    let meta = std::fs::metadata(&wasm_path).with_context(|| format!("{}", wasm_path.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);

    let mp_key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (patch::VERSION, mp::COMMANDS).hash(&mut h);
        h.finish() as u32
    };
    let cache = startup.cache_dir.join(format!("game-{}-{stamp}-{mp_key:08x}.cwasm", meta.len()));

    if let Some(m) = cache.exists().then(|| unsafe { Module::deserialize_file(engine, &cache) }.ok()).flatten() {
        return Ok(m);
    }
    eprintln!("[host] compiling {} to native code (one-time)...", wasm_path.display());
    let m = compile_patched(engine, &wasm_path)?;
    std::fs::write(&cache, m.serialize()?)?;
    Ok(m)
}

#[cfg(feature = "jit")]
fn compile_patched(engine: &Engine, wasm_path: &std::path::Path) -> Result<Module> {
    let wasm = std::fs::read(wasm_path).with_context(|| format!("{}", wasm_path.display()))?;
    let t = Instant::now();
    let patched = patch::patch(&wasm, mp::COMMANDS)?;
    eprintln!("[host] multiplayer hooks added in {:?}", t.elapsed());
    Module::new(engine, &patched).map_err(anyhow::Error::from).context("compile module")
}

pub fn instantiate(rt: &Arc<Runtime>, pthread: u64) -> Result<(Store<HostState>, Instance)> {
    let mut store = Store::new(&rt.engine, HostState { pthread });
    let mut linker: Linker<HostState> = Linker::new(&rt.engine);
    linker.define(&store, "env", "memory", rt.mem.clone())?;
    for (idx, imp) in rt.imports.iter().enumerate() {
        linker.func_new(&imp.module, &imp.name, imp.ty.clone(), move |caller, params, results| {
            host::call(caller, idx, params, results).map_err(wasmtime::Error::from_anyhow)
        })?;
    }
    let instance = linker.instantiate(&mut store, &rt.module)?;
    Ok((store, instance))
}

pub fn run_engine(rt: &Arc<Runtime>, args: Vec<String>) -> Result<()> {
    let (mut store, instance) = instantiate(rt, 0)?;
    call_export(&mut store, &instance, "_emscripten_tls_init", &[])?;
    call_export(&mut store, &instance, "__wasm_call_ctors", &[])?;
    let argv = build_argv(&mut store, &instance, &rt.mem, &args)?;
    eprintln!("[host] main: {}", args.join(" "));
    let main = instance.get_func(&mut store, "__main_argc_argv").context("no main")?;
    let mut out = [Val::I32(0)];
    match main.call(&mut store, &[Val::I32(args.len() as i32), Val::I64(argv as i64)], &mut out) {
        Ok(()) => eprintln!("[host] main returned {:?}", out[0]),
        Err(_) if host::take_unwind() => {
            eprintln!("[host] main unwound with a live runtime; waiting on engine threads");
            loop { std::thread::park(); }
        }
        Err(e) => eprintln!("[host] main trapped: {e:?}"),
    }
    Ok(())
}

pub fn call_export(store: &mut Store<HostState>, inst: &Instance, name: &str, args: &[Val]) -> Result<Vec<Val>> {
    let f = inst.get_func(&mut *store, name).with_context(|| format!("no export {name}"))?;
    let ty = f.ty(&mut *store);
    let args = coerce_args(args, &ty);
    let mut results: Vec<Val> = ty.results().map(|t| default_val(&t)).collect();
    f.call(&mut *store, &args, &mut results)?;
    Ok(results)
}

pub fn coerce_args(args: &[Val], ty: &FuncType) -> Vec<Val> {
    ty.params()
        .zip(args)
        .map(|(pt, v)| {
            let i = match v { Val::I32(x) => *x as i64, Val::I64(x) => *x, other => return other.clone() };
            match pt { ValType::I32 => Val::I32(i as i32), ValType::I64 => Val::I64(i), _ => v.clone() }
        })
        .collect()
}

fn build_argv(store: &mut Store<HostState>, inst: &Instance, mem: &SharedMemory, args: &[String]) -> Result<u64> {
    let alloc = inst.get_typed_func::<i64, i64>(&mut *store, "_emscripten_stack_alloc")?;
    let mut ptrs = Vec::new();
    for a in args {
        let bytes = [a.as_bytes(), &[0]].concat();
        let p = alloc.call(&mut *store, bytes.len() as i64)? as u64;
        write_bytes(mem, p, &bytes);
        ptrs.push(p);
    }
    let argv = alloc.call(&mut *store, ((ptrs.len() + 1) * 8) as i64)? as u64;
    for (i, p) in ptrs.iter().chain(std::iter::once(&0)).enumerate() {
        write_u64(mem, argv + i as u64 * 8, *p);
    }
    Ok(argv)
}

pub fn default_val(t: &ValType) -> Val {
    match t {
        ValType::I32 => Val::I32(0),
        ValType::I64 => Val::I64(0),
        ValType::F32 => Val::F32(0),
        ValType::F64 => Val::F64(0),
        ValType::V128 => Val::V128(0u128.into()),
        ValType::Ref(_) => Val::null_any_ref(),
    }
}

pub fn mem_base_len(mem: &SharedMemory) -> (*mut u8, usize) {
    let d = mem.data();
    (d.as_ptr() as *mut u8, d.len())
}
pub fn read_u64(mem: &SharedMemory, addr: u64) -> Option<u64> {
    let (base, len) = mem_base_len(mem);
    let a = addr as usize;
    if a + 8 > len { return None; }
    unsafe { Some(std::ptr::read_unaligned(base.add(a) as *const u64)) }
}
pub fn read_u32(mem: &SharedMemory, addr: u64) -> Option<u32> {
    let (base, len) = mem_base_len(mem);
    let a = addr as usize;
    if a + 4 > len { return None; }
    unsafe { Some(std::ptr::read_unaligned(base.add(a) as *const u32)) }
}
pub fn write_u64(mem: &SharedMemory, addr: u64, v: u64) -> bool {
    write_bytes(mem, addr, &v.to_le_bytes())
}
pub fn write_u32(mem: &SharedMemory, addr: u64, v: u32) -> bool {
    write_bytes(mem, addr, &v.to_le_bytes())
}
pub fn read_bytes(mem: &SharedMemory, addr: u64, n: u64) -> Vec<u8> {
    let (base, len) = mem_base_len(mem);
    let (a, n) = (addr as usize, n as usize);
    if a + n > len { return Vec::new(); }
    unsafe { std::slice::from_raw_parts(base.add(a), n).to_vec() }
}
pub fn write_bytes(mem: &SharedMemory, addr: u64, data: &[u8]) -> bool {
    let (base, len) = mem_base_len(mem);
    let a = addr as usize;
    if a + data.len() > len { return false; }
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), base.add(a), data.len()); }
    true
}
pub fn read_cstr(mem: &SharedMemory, addr: u64) -> String {
    let (base, len) = mem_base_len(mem);
    let mut a = addr as usize;
    let mut out = Vec::new();
    while a < len && out.len() < 65536 {
        let b = unsafe { *base.add(a) };
        if b == 0 { break; }
        out.push(b);
        a += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(feature = "jit")]
mod patch {
    use crate::mp;
    use anyhow::Result;
    use std::collections::HashMap;
    use wasmparser::{Operator, Parser, Validator};
    use wasm_encoder::{Function, Instruction, Module as WasmModule, RawSection};

    pub const VERSION: u32 = 3;

    #[derive(Clone)]
    struct ImportedFunc { module: String, name: String, params: Vec<wasmparser::ValType>, results: Vec<wasmparser::ValType> }

    pub fn patch(wasm: &[u8], commands: &[mp::Command]) -> Result<Vec<u8>> {
        let mut imports = Vec::new();
        let mut func_count = 0;
        let mut export_map = HashMap::new();

        for payload in Parser::new(0).parse_all(wasm) {
            match payload? {
                wasmparser::Payload::ImportSection(s) => {
                    for imp in s {
                        let imp = imp?;
                        if let wasmparser::TypeRef::Func(type_idx) = imp.ty {
                            imports.push(ImportedFunc {
                                module: imp.module.to_string(),
                                name: imp.name.to_string(),
                                params: Vec::new(),
                                results: Vec::new(),
                            });
                        }
                    }
                }
                wasmparser::Payload::TypeSection(s) => { /* skip */ }
                wasmparser::Payload::FunctionSection(s) => {
                    for f in s { let _ = f?; func_count += 1; }
                }
                wasmparser::Payload::ExportSection(s) => {
                    for exp in s {
                        let exp = exp?;
                        if let wasmparser::ExternalKind::Func = exp.kind {
                            export_map.insert(exp.name.to_string(), exp.index);
                        }
                    }
                }
                _ => {}
            }
        }

        let mut module = WasmModule::new();
        module.section(&RawSection { id: 0, data: &wasm[0..0], range: 0..0 });
        module.section(&RawSection { id: 1, data: &wasm[0..0], range: 0..0 });

        let mut validator = Validator::new_with_features(wasmparser::WasmFeatures::all());
        let mut parser = Parser::new(0);
        let mut in_function = false;
        let mut local_decls = Vec::new();

        for payload in parser.parse_all(wasm) {
            let payload = payload?;
            match payload {
                wasmparser::Payload::Version { num, encoding, range } => {
                    module.section(&RawSection { id: wasmparser::SectionId::Custom as u8, data: &wasm[range.start..range.end], range });
                }
                wasmparser::Payload::End(range) => {
                    module.section(&RawSection { id: wasmparser::SectionId::Custom as u8, data: &wasm[range.start..range.end], range });
                }
                wasmparser::Payload::CodeSectionEntry(func_body) => {
                    in_function = true;
                    local_decls.clear();
                    let mut reader = func_body.get_binary_reader();
                    let count = reader.read_var_u32()?;
                    for _ in 0..count {
                        let n = reader.read_var_u32()?;
                        let t = reader.read_type()?;
                        local_decls.push((n, t));
                    }
                }
                wasmparser::Payload::CodeSectionEntryBody(body) if in_function => {
                    let mut func = Function::new(local_decls.iter().map(|(n, t)| (*n, *t)).collect());
                    let mut reader = body.get_binary_reader();
                    let end = reader.original_position() + body.range().len();
                    while reader.original_position() < end {
                        let op = reader.read_operator()?;
                        match op {
                            Operator::Call { function_index } => {
                                if let Some(name) = export_map.get(&format!("func_{function_index}")) {
                                    if mp::COMMANDS.iter().any(|c| c.name == *name) {
                                        func.instruction(&Instruction::Call(*name));
                                        continue;
                                    }
                                }
                                func.instruction(&Instruction::Call(function_index));
                            }
                            _ => func.instruction(&Instruction::from_operator(&op)?),
                        }
                    }
                    module.section(&wasm_encoder::CodeSection::new(&[&func]));
                    in_function = false;
                }
                _ => {
                    let range = payload.range();
                    module.section(&RawSection { id: payload.as_section_id().unwrap_or(0), data: &wasm[range.start..range.end], range });
                }
            }
        }

        Ok(module.finish())
    }
}

struct AndroidSurface {
    native_window: *mut ndk_sys::ANativeWindow,
    width: u32,
    height: u32,
}

unsafe impl Send for AndroidSurface {}
unsafe impl Sync for AndroidSurface {}

impl HasRawDisplayHandle for AndroidSurface {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        let mut handle = raw_window_handle::AndroidDisplayHandle::new();
        handle.ndk = 1;
        RawDisplayHandle::Android(handle)
    }
}

impl HasRawWindowHandle for AndroidSurface {
    fn raw_window_handle(&self) -> RawWindowHandle {
        let mut handle = raw_window_handle::AndroidWindowHandle::new();
        handle.a_native_window = self.native_window as *mut std::ffi::c_void;
        RawWindowHandle::Android(handle)
    }
}

static SURFACE_READY: AtomicBool = AtomicBool::new(false);
static mut SURFACE_PTR: Option<*mut AndroidSurface> = None;

fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug)
    );
    log::info!("Muguet Android starting");

    // Store JavaVM for JNI calls
    let vm = app.vm();
    JAVA_VM.set(vm.clone()).ok();

    let mut startup = match config::setup() {
        Ok(s) => s,
        Err(e) => {
            log::error!("[config] {e:#}");
            return;
        }
    };

    // Override paths for Android
    startup.wasm_path = std::path::PathBuf::from("/data/local/tmp/muguet/game.cwasm");
    startup.cache_dir = std::path::PathBuf::from("/data/local/tmp/muguet");
    std::env::set_var("MUGUET_DATA", "/data/local/tmp/muguet/game/data");
    std::env::set_var("MUGUET_SHADERS", "/data/local/tmp/muguet/game/b/shaders");
    std::env::set_var("MUGUET_FSROOT", "/data/local/tmp/muguet/saves");

    // Apply launch config from Java
    if let Some(cfg) = LAUNCH_CONFIG.get() {
        std::env::set_var("MUGUET_START", cfg.start_mode.to_string());
        std::env::set_var("MUGUET_SCALE", cfg.scale.to_string());
        std::env::set_var("MUGUET_FPS", cfg.fps_limit.to_string());
        std::env::set_var("MUGUET_TOUCH_SENS", cfg.touch_sens.to_string());
        std::env::set_var("MUGUET_FULLSCREEN", if cfg.fullscreen { "1" } else { "0" });
        std::env::set_var("MUGUET_MUTE", if cfg.mute { "1" } else { "0" });
        if cfg.multiplayer {
            std::env::set_var("MUGUET_MULTIPLAYER", "1");
            if cfg.host {
                std::env::set_var("MUGUET_HOST", "1");
            } else if !cfg.server_address.is_empty() {
                std::env::set_var("MUGUET_SERVER", &cfg.server_address);
            }
            if !cfg.player_name.is_empty() {
                std::env::set_var("MUGUET_NAME", &cfg.player_name);
            }
        }
        eprintln!("[launch] applied config: start_mode={}, scale={}, fps={}, touch_sens={}, multiplayer={}", 
            cfg.start_mode, cfg.scale, cfg.fps_limit, cfg.touch_sens, cfg.multiplayer);
    }

    let rt = match make_runtime(&startup) {
        Ok(rt) => rt,
        Err(e) => {
            log::error!("[host] {e:#}");
            return;
        }
    };

    let mut args = vec![
        "./this.program".to_string(),
        "-rootdir=/game/".to_string(),
        "-nodisplaycalibration".to_string(),
        "-noSocialClub".to_string(),
        "-pc:nosocialclub".to_string(),
        "-nonetwork".to_string(),
        "-output".to_string(),
        "-forceResolution".to_string(),
        "-Script_all=warning".to_string(),
        "-replay_all=warning".to_string(),
        "-fastscriptcore".to_string(),
    ];
    args.extend(startup.engine_args.clone());

    let surface_created = Arc::new(AtomicBool::new(false));
    let surface_ready = surface_created.clone();

    std::thread::Builder::new()
        .name("engine-main".into())
        .stack_size(64 << 20)
        .spawn(move || {
            while !surface_ready.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(100));
            }
            let (w, h) = unsafe {
                if let Some(ptr) = SURFACE_PTR {
                    let s = &*ptr;
                    (s.width, s.height)
                } else { (1280, 720) }
            };
            input::set_size(w as i32, h as i32);
            args.extend(["-wgpu".to_string(), format!("-width={w}"), format!("-height={h}")]);
            if let Err(e) = run_engine(&rt, args) {
                log::error!("[host] engine failed: {e:?}");
            }
        })
        .expect("spawn engine thread");

    app.main_loop(|app, event| {
        match event {
            MainEvent::Init => {
                log::info!("MainEvent::Init");
            }
            MainEvent::Resume => {
                log::info!("MainEvent::Resume");
                // Initialize touch overlay
                init_touch_overlay(app);
                // Resume GPU
                crate::gpu::SUSPENDED.store(false, Ordering::SeqCst);
                // Resume audio
                crate::audio::resume(
                    std::env::var("MUGUET_AUDIO_RING").unwrap_or("0".into()).parse().unwrap_or(0),
                    std::env::var("MUGUET_AUDIO_CAPACITY").unwrap_or("8820".into()).parse().unwrap_or(8820),
                );
            }
            MainEvent::WindowCreated(window) => {
                log::info!("MainEvent::WindowCreated");
                let native_window = window.native_window();
                let surface = AndroidSurface {
                    native_window,
                    width: 1280,
                    height: 720,
                };
                let boxed = Box::new(surface);
                let ptr = Box::into_raw(boxed);
                unsafe { SURFACE_PTR = Some(ptr); }

                let mut desc = InstanceDescriptor::new_without_display_handle();
                desc.backends = Backends::VULKAN;
                let instance = wgpu::Instance::new(desc);
                let surface = instance.create_surface(app.clone()).expect("create surface");
                
                let handoff = gpu::SurfaceHandoff {
                    instance,
                    surface: unsafe { std::mem::transmute(surface) },
                    window: std::sync::Arc::new(unsafe { std::mem::transmute_copy(&ptr) }),
                };
                *gpu::SURFACE.lock().unwrap() = Some(handoff);
                SURFACE_READY.store(true, Ordering::SeqCst);
            }
            MainEvent::WindowResized(window) => {
                let native_window = window.native_window();
                let (width, height) = unsafe {
                    (ndk_sys::ANativeWindow_getWidth(native_window), ndk_sys::ANativeWindow_getHeight(native_window))
                };
                log::info!("MainEvent::WindowResized {}x{}", width, height);
                unsafe {
                    if let Some(ptr) = SURFACE_PTR {
                        (*ptr).width = width as u32;
                        (*ptr).height = height as u32;
                    }
                }
                input::set_size(width as i32, height as i32);
                
                // Notify touch overlay of resize
                if let Some(vm) = JAVA_VM.get() {
                    let mut env = vm.attach_current_thread().unwrap();
                    if let Some(overlay) = TOUCH_OVERLAY.get() {
                        let _ = env.call_method(overlay, "nativeResize", "(IIF)V", &[
                            JValue::from(width as jint),
                            JValue::from(height as jint),
                            JValue::from(1.0f32),
                        ]);
                    }
                }
            }
            MainEvent::WindowDestroyed => {
                log::info!("MainEvent::WindowDestroyed");
                SURFACE_READY.store(false, Ordering::SeqCst);
            }
            MainEvent::Input(input_event) => {
                match input_event.event_type() {
                    android_activity::InputEventType::MotionEvent => {
                        if let Some(motion) = input_event.downcast::<android_activity::MotionEvent>() {
                            let action = motion.action();
                            let pointer_count = motion.pointer_count();
                            
                            for i in 0..pointer_count {
                                let pointer_id = motion.pointer_id(i);
                                let x = motion.get_x(i);
                                let y = motion.get_y(i);
                                
                                // Forward to touch overlay
                                if let Some(vm) = JAVA_VM.get() {
                                    let mut env = vm.attach_current_thread().unwrap();
                                    if let Some(overlay) = TOUCH_OVERLAY.get() {
                                        let _ = env.call_method(overlay, "nativeTouch", "(JFFI)V", &[
                                            JValue::from(pointer_id as jlong),
                                            JValue::from(x),
                                            JValue::from(y),
                                            JValue::from(action as jint),
                                        ]);
                                    }
                                }
                                
                                // Also handle basic mouse input for compatibility
                                match action {
                                    MotionEventAction::Down | MotionEventAction::Move => {
                                        input::mouse_pos(x.round() as i32, y.round() as i32);
                                        input::button(0, true);
                                    }
                                    MotionEventAction::Up => {
                                        input::button(0, false);
                                    }
                                    _ => {}
                                }
                            }
                            return InputStatus::Handled;
                        }
                    }
                    android_activity::InputEventType::KeyEvent => {
                        if let Some(key) = input_event.downcast::<android_activity::KeyEvent>() {
                            let action = key.action();
                            let keycode = key.key_code();
                            let name = format!("Keycode({keycode})");
                            if action == KeyEventAction::Down {
                                input::key(&name, true, false, None, false);
                            } else {
                                input::key(&name, false, false, None, false);
                            }
                            return InputStatus::Handled;
                        }
                    }
                    _ => {}
                }
                ()
            }
            MainEvent::Pause => {
                log::info!("MainEvent::Pause");
                // Suspend GPU
                crate::gpu::SUSPENDED.store(true, Ordering::SeqCst);
                // Pause audio
                crate::audio::pause();
            }
            MainEvent::Destroy => {
                log::info!("MainEvent::Destroy");
                // Pause audio on destroy
                crate::audio::pause();
                mp::shutdown();
                if let Some(vm) = JAVA_VM.get() {
                    let mut env = vm.attach_current_thread().unwrap();
                    if let Some(overlay) = TOUCH_OVERLAY.get() {
                        let _ = env.call_method(overlay, "nativeRelease", "()V", &[]);
                    }
                }
                unsafe {
                    if let Some(ptr) = SURFACE_PTR {
                        let _ = Box::from_raw(ptr);
                        SURFACE_PTR = None;
                    }
                }
            }
            _ => {}
        }
    });
}

fn init_touch_overlay(app: &AndroidApp) {
    if let Some(vm) = JAVA_VM.get() {
        let mut env = vm.attach_current_thread().unwrap();
        
        // Create TouchOverlay instance
        let overlay_class = env.find_class("com/muguet/TouchOverlay").unwrap();
        let overlay = env.new_object(
            overlay_class,
            "(Landroid/content/Context;)V",
            &[JValue::from(&unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr() as _) })]
        ).unwrap();
        
        // Initialize with window
        // removed
        let _ = env.call_method(
            &overlay,
            "initialize",
            "(Landroid/view/Window;)V",
            &[JValue::from(&window)]
        );
        
        // Store global reference
        let global_ref = env.new_global_ref(&overlay).unwrap();
        TOUCH_OVERLAY.set(unsafe { jni::objects::JObject::from_raw(global_ref.as_obj().as_raw()) }).ok();
        
        log::info!("Touch overlay initialized");
    }
}

#[no_mangle]
fn android_main_wrapper(app: AndroidApp) {
    android_main(app);
}

fn main() {}