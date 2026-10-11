# Muguet

[![Build Android APK](https://github.com/c22dev/muguet/actions/workflows/build-android.yml/badge.svg)](https://github.com/c22dev/muguet/actions/workflows/build-android.yml)
[![Latest Release](https://img.shields.io/github/v/release/c22dev/muguet?include_prereleases&label=latest)](https://github.com/c22dev/muguet/releases)
[![Download APK](https://img.shields.io/badge/download-APK-blue)](https://c22dev.github.io/muguet/)

A native port of GTA V (WebAssembly) for **iOS/macOS** and **Android**, with multiplayer.

## Platforms

| Platform | Status | Install |
|----------|--------|---------|
| **Android** (arm64, API 24+) | ✅ | [Download APK](https://c22dev.github.io/muguet/) \| [Releases](https://github.com/c22dev/muguet/releases) |
| **iOS** (15+, arm64, 8GB RAM) | ✅ | AltStore / Sideloadly |
| **macOS** (Apple Silicon, 14+) | ✅ | `./make-app.sh` |

---

## How it works

The game's WebAssembly binary (`game.wasm`) runs natively via **wasmtime** (AOT on iOS/Android, JIT on macOS). Graphics use **wgpu** → **Vulkan** (Android) / **Metal** (Apple). Multiplayer is a custom UDP layer (port 47520).

---

## Android

### Quick install
1. Download latest `Muguet.apk` from [GitHub Pages](https://c22dev.github.io/muguet/) or [Releases](https://github.com/c22dev/muguet/releases)
2. `adb install Muguet.apk` (or open on device)
3. Launch **Muguet** → **Import game.zip** → **Play**

### Requirements
- Android 7.0+ (API 24)
- arm64 device (all modern phones)
- 6 GB RAM recommended (game uses ~3 GB)
- Vulkan 1.1+ (all arm64 devices since ~2016)

### Features
- **Launcher UI** — import, settings, multiplayer config
- **Touch overlay** — virtual joystick, 19 buttons, nav D-pad, wheel menus
- **Oboe audio** — low-latency, pauses on home, resumes on reopen
- **Lifecycle** — GPU suspend/resume, clean destroy
- **Multiplayer** — host/join via LAN or internet (UDP 47520)

### Build yourself
```bash
# Linux/macOS/WSL2 with NDK r26+
rustup target add aarch64-linux-android
export ANDROID_NDK_HOME=~/Android/Sdk/ndk/26.1.10909125
./build-android.sh /path/to/game.wasm
# Output: build-android/Muguet.apk
```

---

## iOS / macOS

See [original instructions](#building) — requires Apple Silicon, Xcode 26+, Apple Developer account.

### Building
```bash
# macOS
./make-app.sh
# → build/Muguet.app

# iOS
TEAM=<your-team-id> ./make-ios.sh game.zip --install
# or --ipa for AltStore/Sideloadly
```

---

## Game files (all platforms)

You need a `game.zip` containing:
```
game.zip/
├── data/           # RPF archives (from playgta5.com mirror)
└── b/<build>/      # game.wasm + shaders/
    ├── game.wasm
    └── shaders/
        └── index.json
```

**Get it from playgta5.com:**
1. Open https://playgta5.com in browser
2. DevTools (F12) → Network → filter "WASM"
3. Reload → save `game.wasm` (largest file, ~50-100 MB)
4. Also save `shaders/` folder and `shader-index.json`

---

## Multiplayer

- One player **Hosts** (launcher → Play with friends → Host)
- Others **Join** (enter host's IP: `192.168.x.x`)
- **Internet**: forward UDP **47520** on host's router
- Runs `muguet-server` on any machine for dedicated relay

---

## Limitations

| Issue | Platform |
|-------|----------|
| Minimap missing | All |
| Story missions single-player only | All |
| First launch compiles shaders (30-60s) | All |
| No on-screen keyboard | Android |
| Requires 6 GB+ RAM | Android |

---

## License

GPL-3.0-or-later — [LICENSE](LICENSE)

---

## Credits

- **Muguet** by [c22dev](https://github.com/c22dev) — iOS/macOS/Android ports
- **wasmtime** — WebAssembly runtime
- **wgpu** — Cross-platform GPU abstraction
- **GTA V WebAssembly** — Rockstar Games (unofficial port)