// muguet - audio.rs
// Cross-platform audio: CoreAudio (macOS/iOS) + Oboe (Android)
// Single thread with pause/resume control

use crate::RT;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

static UNDERRUN_FRAMES: AtomicU64 = AtomicU64::new(0);
static CALLBACKS: AtomicU64 = AtomicU64::new(0);

enum AudioCmd {
    Pause,
    Resume { ring: u64, capacity: u32 },
    Stop,
}

static AUDIO_TX: Mutex<Option<Sender<AudioCmd>>> = Mutex::new(None);
static AUDIO_THREAD: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);

#[cfg(target_os = "android")]
fn default_host() -> cpal::Host {
    cpal::host_from_id(cpal::HostId::Oboe).unwrap_or_else(|_| cpal::default_host())
}

#[cfg(not(target_os = "android"))]
fn default_host_dup() -> cpal::Host {
    cpal::default_host()
}

pub fn start(ring: u64, capacity: u32) {
    let tx = {
        let mut guard = AUDIO_TX.lock().unwrap();
        if guard.is_some() {
            return; // already running
        }
        let (tx, rx) = channel::<AudioCmd>();
        *guard = Some(tx.clone());
        tx
    };
    
    let handle = thread::Builder::new()
        .name("audio".into())
        .spawn(move || audio_thread(rx, ring, capacity))
        .expect("spawn audio thread");
    
    *AUDIO_THREAD.lock().unwrap() = Some(handle);
}

pub fn pause() {
    if let Some(tx) = AUDIO_TX.lock().unwrap().take() {
        let _ = tx.send(AudioCmd::Pause);
    }
    if let Some(handle) = AUDIO_THREAD.lock().unwrap().take() {
        let _ = handle.join();
    }
}

pub fn resume(ring: u64, capacity: u32) {
    if AUDIO_TX.lock().unwrap().is_some() {
        return; // already running
    }
    let (tx, rx) = channel::<AudioCmd>();
    *AUDIO_TX.lock().unwrap() = Some(tx.clone());
    
    let handle = thread::Builder::new()
        .name("audio".into())
        .spawn(move || audio_thread(rx, ring, capacity))
        .expect("spawn audio thread");
    
    *AUDIO_THREAD.lock().unwrap() = Some(handle);
}

fn audio_thread(rx: Receiver<AudioCmd>, mut ring: u64, mut capacity: u32) {
    let mut stream: Option<cpal::Stream> = None;
    let mut running = false;
    
    let try_start = |ring: u64, capacity: u32| -> Option<cpal::Stream> {
        if std::env::var("MUGUET_MUTE").is_ok() {
            return None;
        }
        let host = default_host();
        let device = host.default_output_device()?;
        let config = cpal::StreamConfig { channels: 2, sample_rate: 48000, buffer_size: cpal::BufferSize::Default };
        let (base, _) = crate::mem_base_len(&RT.get()?.mem);
        let base = base as usize;
        let hdr = move |i: usize| -> &'static AtomicI32 { unsafe { &*((base + ring as usize + 4 * i) as *const AtomicI32) } };
        let data = base + ring as usize + 64;
        let mask = capacity.wrapping_sub(1);
        
        let stream = device.build_output_stream(
            config,
            move |out: &mut [f32], _| {
                let frames = out.len() / 2;
                let read = hdr(1).load(Ordering::SeqCst) as u32;
                let write = hdr(0).load(Ordering::SeqCst) as u32;
                let take = (write.wrapping_sub(read) as usize).min(frames);
                for i in 0..take {
                    let at = ((read.wrapping_add(i as u32) & mask) * 2) as usize;
                    unsafe {
                        let p = (data as *const f32).add(at);
                        out[2 * i] = *p;
                        out[2 * i + 1] = *p.add(1);
                    }
                }
                for s in &mut out[take * 2..] { *s = 0.0; }
                UNDERRUN_FRAMES.fetch_add((frames - take) as u64, Ordering::Relaxed);
                CALLBACKS.fetch_add(1, Ordering::Relaxed);
                hdr(1).store(read.wrapping_add(take as u32) as i32, Ordering::SeqCst);
                hdr(3).fetch_add(1, Ordering::SeqCst);
            },
            |e| eprintln!("[audio] stream error: {e}"),
            None,
        ).ok()?;
        stream.as_ref()?.play().ok()?;
        
        let backend = if cfg!(target_os = "android") { "Oboe" } else { "CoreAudio" };
        eprintln!("[audio] {backend} started (ring 0x{ring:x}, {capacity} frames)");
        
        // Stats thread
        std::thread::spawn(move || {
            let mut last_w = hdr(0).load(Ordering::SeqCst) as u32;
            let mut last_u = 0;
            let mut last_c = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(5));
                let w = hdr(0).load(Ordering::SeqCst) as u32;
                let (u, c) = (UNDERRUN_FRAMES.load(Ordering::Relaxed), CALLBACKS.load(Ordering::Relaxed));
                eprintln!("[audio] mixer produced {:.0} frames/s (needs 48000), underrun {:.0} frames/s, {} callbacks/s, ring fill {}",
                    w.wrapping_sub(last_w) as f64 / 5.0, (u - last_u) as f64 / 5.0, (c - last_c) / 5, w.wrapping_sub(hdr(1).load(Ordering::SeqCst) as u32));
                last_w = w; last_u = u; last_c = c;
            }
        });
        
        Some(stream)
    };
    
    // Initial start
    stream = try_start(ring, capacity);
    running = stream.is_some();
    
    loop {
        match rx.recv() {
            Ok(AudioCmd::Pause) => {
                if let Some(s) = stream.take() {
                    let _ = s.pause();
                }
                running = false;
                eprintln!("[audio] paused");
            }
            Ok(AudioCmd::Resume { ring: new_ring, capacity: new_cap }) => {
                ring = new_ring;
                capacity = new_cap;
                stream = try_start(ring, capacity);
                running = stream.is_some();
            }
            Ok(AudioCmd::Stop) | Err(_) => {
                if let Some(s) = stream.take() {
                    let _ = s.pause();
                }
                eprintln!("[audio] stopped");
                break;
            }
        }
    }
}

pub fn stop() {
    if let Some(tx) = AUDIO_TX.lock().unwrap().take() {
        let _ = tx.send(AudioCmd::Stop);
    }
    if let Some(handle) = AUDIO_THREAD.lock().unwrap().take() {
        let _ = handle.join();
    }
}

#[cfg(target_os = "android")]
fn default_host_dup() -> cpal::Host {
    cpal::host_from_id(cpal::HostId::Oboe).unwrap_or_else(|_| cpal::default_host())
}

#[cfg(not(target_os = "android"))]
fn default_host_dup() -> cpal::Host {
    cpal::default_host()
}
