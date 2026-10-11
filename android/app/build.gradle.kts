plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("com.github.aimetal.rust-android-gradle") version "0.9.0" apply false
}

android {
    namespace = "com.muguet"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.muguet"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"

        // Native library configuration
        ndk {
            abiFilters.add("arm64-v8a")
        }

        // Large heap for WASM memory (2GB+)
        manifestPlaceholders["largeHeap"] = "true"
        
        // Vector drawables for Compose
        vectorDrawables.useSupportLibrary = true
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.10"
    }

    // External native build (for Rust)
    externalNativeBuild {
        create("cargo") {
            path = "../../Cargo.android.toml"
            targets("muguet-android")
            buildType = "release"
            args = listOf("--target", "aarch64-linux-android")
        }
    }

    // Packaging options for native libs
    packagingOptions {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    // Core
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.11.0")
    implementation("androidx.activity:activity-ktx:1.8.2")
    implementation("androidx.constraintlayout:constraintlayout:2.1.4")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.7.0")
    
    // Jetpack Compose
    val composeBom = platform("androidx.compose:compose-bom:2024.02.00")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.runtime:runtime-livedata")
    implementation("androidx.activity:activity-compose:1.8.2")
    
    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.7.3")
}