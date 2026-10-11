// muguet - touch_android.rs
// Android touch overlay using JNI + Android View system
// Mirrors iOS touch.rs functionality: virtual joystick, action buttons, nav D-pad, look drag

use crate::input;
use crate::pad;
use crate::gpu;
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jfloat};
use jni::JNIEnv;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, LazyLock};

// Button action types (mirroring iOS Act enum)
#[derive(Clone, Copy, PartialEq)]
enum Act {
    Key(&'static str),
    Mouse(u8),
    AimToggle,
    KeyNav(&'static str),
    NavToggle,
    Horn,
}

// Anchor positions (mirroring iOS Anchor enum)
#[derive(Clone, Copy)]
enum Anchor {
    BottomRight(f64, f64),
    TopLeft(f64, f64),
    TopRight(f64, f64),
    Nav(f64, f64),
}

struct Spec {
    anchor: Anchor,
    radius: f64,
    on_foot: Option<(&'static str, Act)>,
    in_car: Option<(&'static str, Act)>,
    nav: bool,
}

const fn b(anchor: Anchor, radius: f64, on_foot: Option<(&'static str, Act)>, in_car: Option<(&'static str, Act)>) -> Spec {
    Spec { anchor, radius, on_foot, in_car, nav: false }
}
const fn n(x: f64, label: &'static str, key: &'static str) -> Spec {
    Spec { anchor: Anchor::Nav(x, 0.0), radius: 21.0, on_foot: Some((label, Act::Key(key))), in_car: Some((label, Act::Key(key))), nav: true }
}

use Act::*;
use Anchor::*;

const BUTTONS: &[Spec] = &[
    b(BottomRight(62.0, 68.0), 40.0, Some(("FIRE", Mouse(1))), Some(("GAS", Key("KeyW")))),
    b(BottomRight(158.0, 46.0), 31.0, Some(("AIM", AimToggle)), Some(("BRAKE", Key("KeyS")))),
    b(BottomRight(58.0, 168.0), 28.0, Some(("JUMP", Key("Space"))), Some(("H.BRAKE", Key("Space")))),
    b(BottomRight(140.0, 132.0), 26.0, Some(("ENTER", Key("KeyF"))), Some(("EXIT", Key("KeyF")))),
    b(BottomRight(222.0, 98.0), 24.0, Some(("RELOAD", Key("KeyR"))), Some(("HORN", Horn))),
    b(TopRight(30.0, 30.0), 22.0, Some(("WEAPON", Key("Tab"))), Some(("WEAPON", Key("Tab")))),
    b(TopRight(84.0, 30.0), 22.0, Some(("COVER", Key("KeyQ"))), Some(("RADIO", Key("KeyQ")))),
    b(TopRight(138.0, 30.0), 22.0, Some(("CROUCH", Key("ControlLeft"))), Some(("LOOK\nBACK", Key("KeyC")))),
    b(TopRight(192.0, 30.0), 22.0, Some(("CAM", Key("KeyV"))), Some(("CAM", Key("KeyV")))),
    b(TopLeft(30.0, 30.0), 22.0, Some(("PAUSE", KeyNav("Escape"))), Some(("PAUSE", KeyNav("Escape")))),
    b(TopLeft(84.0, 30.0), 22.0, Some(("PHONE", KeyNav("ArrowUp"))), Some(("PHONE", KeyNav("ArrowUp")))),
    b(TopLeft(138.0, 30.0), 22.0, Some(("MENU\nKEYS", NavToggle)), Some(("MENU\nKEYS", NavToggle))),
    b(TopLeft(192.0, 30.0), 22.0, Some(("SWITCH", Key("AltLeft"))), Some(("SWITCH", Key("AltLeft")))),
    n(-135.0, "◀", "ArrowLeft"),
    n(-87.0, "▲", "ArrowUp"),
    n(-39.0, "▼", "ArrowDown"),
    n(9.0, "▶", "ArrowRight"),
    n(63.0, "OK", "Enter"),
    n(117.0, "BACK", "Backspace"),
];

const STICK_RADIUS: f64 = 58.0;
const KNOB_RADIUS: f64 = 26.0;
const LOOK_GAIN: f64 = 3.0;
const WHEEL_DEADZONE: f64 = 10.0;
const WHEEL_FULL: f64 = 45.0;
const WHEEL_PUSH: f64 = 40.0;

static TOUCH_SENS: LazyLock<f64> = LazyLock::new(|| std::env::var("MUGUET_TOUCH_SENS").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0));
const MIN_HOLD: std::time::Duration = std::time::Duration::from_millis(120);

enum Grip {
    Stick { origin: (f64, f64), at: (f64, f64) },
    Look { last: (f64, f64) },
    Button { idx: usize, start: (f64, f64), last: (f64, f64) },
}

struct ButtonView {
    view_id: jlong,
    label_id: jlong,
    center: (f64, f64),
    radius: f64,
}

struct Overlay {
    root_id: jlong,
    buttons: Vec<ButtonView>,
    stick_base_id: jlong,
    stick_knob_id: jlong,
    stick_home: (f64, f64),
    safe: (f64, f64, f64, f64),
    scale: f64,
    grips: HashMap<i64, Grip>,  // touch id -> grip
    held: HashSet<&'static str>,
    since: HashMap<&'static str, std::time::Instant>,
    mouse_since: [std::time::Instant; 2],
    lingering: bool,
    mouse: u8,
    aim_latched: bool,
    nav: bool,
    in_car: bool,
    hidden: bool,
}

static OVERLAY: LazyLock<Arc<Mutex<Option<Overlay>>>> = LazyLock::new(|| Arc::new(Mutex::new(None)));

// JNI helper functions
fn jni_call(env: &mut JNIEnv, obj: &JObject, method: &str, sig: &str, args: &[JValue]) -> jni::errors::Result<JValue> {
    env.call_method(obj, method, sig, args)
}

fn jni_new_object(env: &mut JNIEnv, class: &str, sig: &str, args: &[JValue]) -> jni::errors::Result<JObject> {
    env.new_object(class, sig, args)
}

fn dp_to_px(env: &mut JNIEnv, context: &JObject, dp: f64) -> f64 {
    let display_metrics = jni_call(env, context, "getResources", "()Landroid/content/res/Resources;", &[])
        .and_then(|v| jni_call(env, &v.l().unwrap(), "getDisplayMetrics", "()Landroid/util/DisplayMetrics;", &[]))
        .ok();
    if let Some(dm) = display_metrics {
        let density = env.get_field(&dm.l().unwrap(), "density", "F").ok().map(|v| v.f().unwrap()).unwrap_or(1.0);
        dp * density as f64
    } else {
        dp
    }
}

impl Overlay {
    fn new(env: &mut JNIEnv, context: &JObject, window: &JObject, scale: f64) -> Self {
        // Get screen size
        let display = jni_call(env, window, "getWindowManager", "()Landroid/view/WindowManager;", &[])
            .and_then(|v| jni_call(env, &v.l().unwrap(), "getDefaultDisplay", "()Landroid/view/Display;", &[]))
            .and_then(|v| jni_call(env, &v.l().unwrap(), "getRealSize", "(Landroid/graphics/Point;)V", &[JValue::from(JObject::null())]))
            .ok();
        
        // For now use default size, will be updated on resize
        let (full_w, full_h) = (1280.0 / scale, 720.0 / scale);
        let (sx, sy, sw, sh) = (0.0, 0.0, full_w, full_h); // safe area
        
        // Create root view
        let view_class = env.find_class("android/view/View").unwrap();
        let root = jni_new_object(env, "android/view/View", "(Landroid/content/Context;)V", &[JValue::from(context)]).unwrap();
        let root_id = env.get_field(&root, "mNativeView", "J").unwrap().j().unwrap();
        
        // Set root view properties
        let _ = jni_call(env, &root, "setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V", &[
            JValue::from(jni_new_object(env, "android/view/ViewGroup$LayoutParams", 
                "(II)V", &[JValue::from(full_w as i32), JValue::from(full_h as i32)]).unwrap())
        ]);
        
        let place = |a: Anchor| -> (f64, f64) {
            match a {
                Anchor::BottomRight(dx, dy) => (sx + sw - dx, sy + sh - dy),
                Anchor::TopLeft(dx, dy) => (sx + dx, sy + dy),
                Anchor::TopRight(dx, dy) => (sx + sw - dx, sy + dy),
                Anchor::Nav(dx, dy) => (sx + sw / 2.0 + dx, sy + 30.0 + dy),
            }
        };
        
        let mut buttons = Vec::new();
        for s in BUTTONS {
            let center = place(s.anchor);
            let (cx, cy) = center;
            let r = s.radius;
            
            // Create button view
            let btn = jni_new_object(env, "android/view/View", "(Landroid/content/Context;)V", &[JValue::from(context)]).unwrap();
            let btn_id = env.get_field(&btn, "mNativeView", "J").unwrap().j().unwrap();
            
            // Set position and size
            let _ = jni_call(env, &btn, "setX", "(F)V", &[JValue::from((cx - r) as f32)]);
            let _ = jni_call(env, &btn, "setY", "(F)V", &[JValue::from((cy - r) as f32)]);
            let _ = jni_call(env, &btn, "setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V", &[
                JValue::from(jni_new_object(env, "android/view/ViewGroup$LayoutParams", 
                    "(II)V", &[JValue::from((2.0 * r) as i32), JValue::from((2.0 * r) as i32)]).unwrap())
            ]);
            
            // Background color (semi-transparent white)
            let _ = jni_call(env, &btn, "setBackgroundColor", "(I)V", &[JValue::from(0x2EFFFFFF)]); // 0.18 alpha
            let _ = jni_call(env, &btn, "setClickable", "(Z)V", &[JValue::from(false as jboolean)]);
            
            // Create label TextView
            let label = jni_new_object(env, "android/widget/TextView", "(Landroid/content/Context;)V", &[JValue::from(context)]).unwrap();
            let label_id = env.get_field(&label, "mNativeView", "J").unwrap().j().unwrap();
            
            let font_size = (r * 0.42).clamp(9.0, 15.0);
            let _ = jni_call(env, &label, "setTextSize", "(F)V", &[JValue::from(font_size as f32)]);
            let _ = jni_call(env, &label, "setTextColor", "(I)V", &[JValue::from(0xE6FFFFFF)]); // white 0.9 alpha
            let _ = jni_call(env, &label, "setGravity", "(I)V", &[JValue::from(17)]); // CENTER
            let _ = jni_call(env, &label, "setMaxLines", "(I)V", &[JValue::from(2)]);
            
            // Add label to button
            let _ = jni_call(env, &btn, "addView", "(Landroid/view/View;)V", &[JValue::from(&label)]);
            
            // Add button to root
            let _ = jni_call(env, &root, "addView", "(Landroid/view/View;)V", &[JValue::from(&btn)]);
            
            buttons.push(ButtonView {
                view_id: btn_id,
                label_id,
                center,
                radius: s.radius,
            });
        }
        
        // Create stick base
        let stick_home = (sx + 40.0 + STICK_RADIUS, sy + sh - 30.0 - STICK_RADIUS);
        let stick_base = jni_new_object(env, "android/view/View", "(Landroid/content/Context;)V", &[JValue::from(context)]).unwrap();
        let stick_base_id = env.get_field(&stick_base, "mNativeView", "J").unwrap().j().unwrap();
        let (shx, shy) = stick_home;
        let _ = jni_call(env, &stick_base, "setX", "(F)V", &[JValue::from((shx - STICK_RADIUS) as f32)]);
        let _ = jni_call(env, &stick_base, "setY", "(F)V", &[JValue::from((shy - STICK_RADIUS) as f32)]);
        let _ = jni_call(env, &stick_base, "setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V", &[
            JValue::from(jni_new_object(env, "android/view/ViewGroup$LayoutParams", 
                "(II)V", &[JValue::from((2.0 * STICK_RADIUS) as i32), JValue::from((2.0 * STICK_RADIUS) as i32)]).unwrap())
        ]);
        let _ = jni_call(env, &stick_base, "setBackgroundColor", "(I)V", &[JValue::from(0x1FFFFF)]); // 0.12 alpha
        let _ = jni_call(env, &root, "addView", "(Landroid/view/View;)V", &[JValue::from(&stick_base)]);
        
        // Create stick knob
        let stick_knob = jni_new_object(env, "android/view/View", "(Landroid/content/Context;)V", &[JValue::from(context)]).unwrap();
        let stick_knob_id = env.get_field(&stick_knob, "mNativeView", "J").unwrap().j().unwrap();
        let _ = jni_call(env, &stick_knob, "setX", "(F)V", &[JValue::from((shx - KNOB_RADIUS) as f32)]);
        let _ = jni_call(env, &stick_knob, "setY", "(F)V", &[JValue::from((shy - KNOB_RADIUS) as f32)]);
        let _ = jni_call(env, &stick_knob, "setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V", &[
            JValue::from(jni_new_object(env, "android/view/ViewGroup$LayoutParams", 
                "(II)V", &[JValue::from((2.0 * KNOB_RADIUS) as i32), JValue::from((2.0 * KNOB_RADIUS) as i32)]).unwrap())
        ]);
        let _ = jni_call(env, &stick_knob, "setBackgroundColor", "(I)V", &[JValue::from(0x4CFFFFFF)]); // 0.3 alpha
        let _ = jni_call(env, &root, "addView", "(Landroid/view/View;)V", &[JValue::from(&stick_knob)]);
        
        let mut o = Overlay {
            root_id: root_id,
            buttons,
            stick_base_id,
            stick_knob_id,
            stick_home,
            safe: (sx, sy, sw, sh),
            scale,
            grips: HashMap::new(),
            held: HashSet::new(),
            since: HashMap::new(),
            mouse_since: [std::time::Instant::now(); 2],
            lingering: false,
            mouse: 0,
            aim_latched: false,
            nav: false,
            in_car: false,
            hidden: false,
        };
        o.relabel();
        eprintln!("[touch] Android overlay ready ({:.0}x{:.0} pt)", full_w, full_h);
        o
    }
    
    fn is_wheel(&self, idx: usize) -> bool {
        match self.spec_act(idx).map(|(_, a)| a) {
            Some(Key("Tab")) | Some(Key("AltLeft")) => true,
            Some(Key("KeyQ")) => self.in_car,
            _ => false,
        }
    }
    
    fn spec_act(&self, idx: usize) -> Option<(&'static str, Act)> {
        let s = &BUTTONS[idx];
        if s.nav && !self.nav { return None; }
        if self.in_car { s.in_car } else { s.on_foot }
    }
    
    fn relabel(&mut self) {
        // In a real implementation, we'd update TextView text via JNI
        // For now, just restyle
        self.restyle();
    }
    
    fn restyle(&self) {
        // Update button alpha based on pressed state
        // Would need JNI calls to setBackgroundColor
    }
    
    fn button_at(&self, p: (f64, f64)) -> Option<usize> {
        (0..self.buttons.len()).filter(|&i| self.spec_act(i).is_some()).find(|&i| {
            let b = &self.buttons[i];
            let r = b.radius + 8.0;
            (p.0 - b.center.0).powi(2) + (p.1 - b.center.1).powi(2) <= r * r
        })
    }
    
    fn in_stick_zone(&self, p: (f64, f64)) -> bool {
        let (sx, sy, sw, _) = self.safe;
        p.0 < sx + sw * 0.4 && p.1 > sy + 70.0
    }
    
    fn handle_touch(&mut self, touch_id: i64, x: f64, y: f64, action: i32) {
        let p = (x / self.scale, y / self.scale);
        
        match action {
            0 => { // ACTION_DOWN
                if let Some(idx) = self.button_at(p) {
                    self.grips.insert(touch_id, Grip::Button { idx, start: p, last: p });
                    match self.spec_act(idx).map(|(_, a)| a) {
                        Some(AimToggle) => self.aim_latched = !self.aim_latched,
                        Some(NavToggle) => { self.nav = !self.nav; self.relabel(); }
                        Some(KeyNav(_)) if !self.nav => { self.nav = true; self.relabel(); }
                        _ => {}
                    }
                } else if self.in_stick_zone(p) && !self.grips.values().any(|g| matches!(g, Grip::Stick { .. })) {
                    self.grips.insert(touch_id, Grip::Stick { origin: p, at: p });
                } else {
                    self.grips.insert(touch_id, Grip::Look { last: p });
                }
            }
            2 => { // ACTION_MOVE
                let wheel = matches!(self.grips.get(&touch_id), Some(Grip::Button { idx, .. }) if self.is_wheel(*idx));
                match self.grips.get_mut(&touch_id) {
                    Some(Grip::Stick { at, .. }) => *at = p,
                    Some(Grip::Button { last, .. }) if wheel => *last = p,
                    Some(Grip::Look { last }) | Some(Grip::Button { last, .. }) => {
                        let (dx, dy) = (p.0 - last.0, p.1 - last.1);
                        *last = p;
                        let speed = (dx * dx + dy * dy).sqrt();
                        let gain = LOOK_GAIN * *TOUCH_SENS * (1.0 + (speed / 12.0).min(1.5));
                        input::mouse_delta_f(dx * gain, dy * gain);
                    }
                    None => {}
                }
            }
            1 | 3 => { // ACTION_UP | ACTION_CANCEL
                self.grips.remove(&touch_id);
            }
            _ => {}
        }
        self.apply();
    }
    
    fn apply(&mut self) {
        let mut keys: HashSet<&'static str> = HashSet::new();
        let mut mouse = if self.aim_latched && !self.in_car { 2u8 } else { 0 };
        let mut stick = None;
        let mut horn = false;
        
        for g in self.grips.values() {
            match g {
                Grip::Stick { origin, at } => stick = Some((*origin, *at)),
                Grip::Button { idx, .. } => match self.spec_act(*idx).map(|(_, a)| a) {
                    Some(Key(k)) | Some(KeyNav(k)) => { keys.insert(k); }
                    Some(Mouse(bit)) => mouse |= bit,
                    Some(Horn) => horn = true,
                    _ => {}
                },
                Grip::Look { .. } => {}
            }
        }
        
        if let Some((o, at)) = stick {
            let (mut dx, mut dy) = (at.0 - o.0, at.1 - o.1);
            let len = (dx * dx + dy * dy).sqrt();
            let k = if len > STICK_RADIUS { STICK_RADIUS / len } else { 1.0 };
            dx *= k;
            dy *= k;
            let (nx, ny) = (dx / STICK_RADIUS, dy / STICK_RADIUS);
            if nx < -0.35 { keys.insert("KeyA"); }
            if nx > 0.35 { keys.insert("KeyD"); }
            if self.in_car {
                if ny < -0.6 { keys.insert("Numpad8"); }
                if ny > 0.6 { keys.insert("Numpad2"); }
            } else {
                if ny < -0.35 { keys.insert("KeyW"); }
                if ny > 0.35 { keys.insert("KeyS"); }
                if len > STICK_RADIUS * 0.95 { keys.insert("ShiftLeft"); }
            }
        }
        
        pad::HORN_TOUCH.store(horn, std::sync::atomic::Ordering::Relaxed);
        
        let now = std::time::Instant::now();
        let young = |t: Option<&std::time::Instant>| t.is_some_and(|t| now - *t < MIN_HOLD);
        self.lingering = false;
        
        for k in self.held.clone() {
            if keys.contains(k) { continue; }
            if young(self.since.get(k)) { keys.insert(k); self.lingering = true; continue; }
            input::key(k, false, false, None, false);
            self.since.remove(k);
        }
        
        for k in keys.difference(&self.held) {
            crate::mp::key_pressed(k);
            input::key(k, true, false, None, false);
            self.since.insert(k, now);
        }
        
        self.held = keys;
        
        for (i, (bit, dom)) in [(1u8, 0usize), (2, 2)].into_iter().enumerate() {
            if self.mouse & bit != 0 && mouse & bit == 0 && young(Some(&self.mouse_since[i])) {
                mouse |= bit;
                self.lingering = true;
            }
            if (mouse ^ self.mouse) & bit != 0 {
                input::button(dom, mouse & bit != 0);
                if mouse & bit != 0 { self.mouse_since[i] = now; }
            }
        }
        self.mouse = mouse;
        self.restyle();
    }
    
    fn release_all(&mut self) {
        self.grips.clear();
        self.aim_latched = false;
        self.apply();
    }
    
    fn update_visibility(&mut self, hide: bool, in_car: bool) {
        if hide != self.hidden {
            self.hidden = hide;
            if hide { self.release_all(); }
            // setVisibility via JNI
        }
        if in_car != self.in_car && !self.hidden {
            self.grips.retain(|_, g| !matches!(g, Grip::Button { .. }));
            self.in_car = in_car;
            if in_car { self.aim_latched = false; }
            self.apply();
            self.relabel();
        }
    }
}

// JNI entry points called from Android Kotlin/Java
#[no_mangle]
pub extern "system" fn Java_com_muguet_TouchOverlay_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    context: JObject,
    window: JObject,
    scale: jfloat,
) {
    let overlay = Overlay::new(&mut env, &context, &window, scale as f64);
    *OVERLAY.lock().unwrap() = Some(overlay);
}

#[no_mangle]
pub extern "system" fn Java_com_muguet_TouchOverlay_nativeTouch(
    mut env: JNIEnv,
    _class: JClass,
    touch_id: jlong,
    x: jfloat,
    y: jfloat,
    action: jint,
) {
    if let Some(mut o) = OVERLAY.lock().unwrap().as_mut() {
        o.handle_touch(touch_id, x as f64, y as f64, action);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_muguet_TouchOverlay_nativeUpdate(
    mut env: JNIEnv,
    _class: JClass,
    hide: jboolean,
    in_car: jboolean,
) {
    if let Some(mut o) = OVERLAY.lock().unwrap().as_mut() {
        o.update_visibility(hide != 0, in_car != 0);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_muguet_TouchOverlay_nativeResize(
    mut env: JNIEnv,
    _class: JClass,
    width: jint,
    height: jint,
    scale: jfloat,
) {
    if let Some(mut o) = OVERLAY.lock().unwrap().as_mut() {
        // Update safe area and reposition views
        let (sx, sy, _, _) = o.safe;
        o.safe = (sx, sy, width as f64 / scale as f64, height as f64 / scale as f64);
        o.scale = scale as f64;
        // Reposition all views - would need JNI calls
    }
}

#[no_mangle]
pub extern "system" fn Java_com_muguet_TouchOverlay_nativeRelease(
    mut env: JNIEnv,
    _class: JClass,
) {
    *OVERLAY.lock().unwrap() = None;
}