#!/bin/bash
# build-android.sh - Build Muguet for Android (aarch64)

set -euo pipefail

GAME_WASM="${1:-}"
if [ -z "$GAME_WASM" ] \vert{}\vert{} [ ! -f "$GAME_WASM" ]; then
    echo "Usage: $0 /path/to/game.wasm"
    exit 1
fi

# Configuration
NDK_VERSION="26.1.10909125"
TARGET="aarch64-linux-android"
API_LEVEL=24
BUILD_DIR="build-android"
GAME_CWASM="$BUILD_DIR/game.cwasm"
OUTPUT_BIN="$BUILD_DIR/muguet-android"
ANDROID_PROJECT_DIR="android"

# Check dependencies
command -v cargo >/dev/null || { echo "cargo not found"; exit 1; }
command -v rustup >/dev/null || { echo "rustup not found"; exit 1; }
command -v wasmtime >/dev/null || { echo "wasmtime not found"; exit 1; }
command -v java >/dev/null || { echo "java not found (need JDK 17+)"; exit 1; }

# Setup NDK
ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/$NDK_VERSION}"
if [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "NDK not found at $ANDROID_NDK_HOME"
    exit 1
fi
export ANDROID_NDK_HOME

# Add Rust target
rustup target add "$TARGET" || true

# Set environment variables for C/C++ compilation (needed for oboe-sys)
TOOLCHAIN_BIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CC_aarch64_linux_android="$TOOLCHAIN_BIN/${TARGET}${API_LEVEL}-clang"
export CXX_aarch64_linux_android="$TOOLCHAIN_BIN/${TARGET}${API_LEVEL}-clang++"
export AR_aarch64_linux_android="$TOOLCHAIN_BIN/llvm-ar"
export PATH="$TOOLCHAIN_BIN:$PATH"

# Configure Cargo linker
mkdir -p .cargo
cat > .cargo/config.toml <<EOF
[target.$TARGET]
linker = "$CC_aarch64_linux_android"
ar = "$AR_aarch64_linux_android"
EOF

# Create build directory
mkdir -p "$BUILD_DIR"

# Copy game assets (data/ + b/<build>/)
GAME_DIR="$BUILD_DIR/game"
mkdir -p "$GAME_DIR"
echo "Copying game assets..."
if [ -d "mirror/playgta5.com/data" ]; then
    cp -r mirror/playgta5.com/data "$GAME_DIR/"
    echo "  Copied data/ from mirror"
else
    echo "  WARNING: mirror/playgta5.com/data not found. You'll need to copy manually."
fi

# Find the build directory with shaders
BUILD_JSON=$(find mirror/playgta5.com -name "index.json" -path "*/shaders/*" 2>/dev/null | head -1 || true)
if [ -n "$BUILD_JSON" ]; then
    BUILD_SOURCE=$(dirname "$(dirname "$BUILD_JSON")")
    mkdir -p "$GAME_DIR/b"
    cp -r "$BUILD_SOURCE" "$GAME_DIR/b/"
    echo "  Copied build/ from $BUILD_SOURCE"
else
    echo "  WARNING: shader build directory not found. You'll need to copy manually."
fi

# AOT compile game.wasm for Android
echo "AOT-compiling game.wasm for Android..."
wasmtime compile \
    --target "$TARGET" \
    --wasm threads=y,memory64=y,bulk-memory=y,multi-memory=y \
    -o "$GAME_CWASM" \
    "$GAME_WASM"

# Build the Android binary (Rust)
echo "Building muguet-android (Rust)..."
cargo build --release \
    --target "$TARGET" \
    --manifest-path Cargo.toml \
    --features jit

# Copy binary
if [ -f "target/$TARGET/release/muguet" ]; then
    cp "target/$TARGET/release/muguet" "$OUTPUT_BIN"
else
    cp "target/$TARGET/release/muguet-android" "$OUTPUT_BIN"
fi
chmod +x "$OUTPUT_BIN"

# Build APK via Gradle
if [ -d "$ANDROID_PROJECT_DIR" ]; then
    echo "Building APK via Gradle..."
    cd "$ANDROID_PROJECT_DIR"
    
    # Copy native library and assets to Android project
    mkdir -p app/src/main/jniLibs/arm64-v8a
    cp "../$OUTPUT_BIN" app/src/main/jniLibs/arm64-v8a/libmuguet-android.so
    
    mkdir -p app/src/main/assets
    cp "../$GAME_CWASM" app/src/main/assets/game.cwasm
    cp -r "../$GAME_DIR" app/src/main/assets/
    
    # Make gradlew executable
    chmod +x gradlew
    
    # Build APK
    ./gradlew assembleRelease
    
    APK_PATH="app/build/outputs/apk/release/app-release.apk"
    if [ -f "$APK_PATH" ]; then
        cp "$APK_PATH" "../$BUILD_DIR/Muguet.apk"
        echo "APK built: $BUILD_DIR/Muguet.apk"
    else
        echo "WARNING: APK not found at expected location"
    fi
    
    cd ..
else
    echo "Android project not found at $ANDROID_PROJECT_DIR, skipping APK build"
fi
