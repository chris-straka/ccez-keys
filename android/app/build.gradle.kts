plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// The Rust core (../../core): built for the phone (arm64) and the
// emulator (x86_64) with cargo-ndk, then read by uniffi-bindgen for the
// Kotlin bindings. Gradle reruns either only when the core changes.
val coreDir = rootProject.layout.projectDirectory.dir("../core")
val dataDir = rootProject.layout.projectDirectory.dir("../data")
val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
val uniffiKotlin = layout.buildDirectory.dir("generated/uniffi")
val coreInputs = listOf("src", "Cargo.toml", "uniffi.toml")
// The workspace's target folder (../target), or CARGO_TARGET_DIR.
val cargoTarget = System.getenv("CARGO_TARGET_DIR")?.let { file(it) } ?: rootProject.layout.projectDirectory.dir("../target").asFile

val buildRustCore by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds the Rust core for arm64-v8a and x86_64"
    workingDir(coreDir)
    coreInputs.forEach { inputs.files(coreDir.file(it).asFile) }
    inputs.dir(dataDir.dir("layouts"))
    outputs.dir(rustJniLibs)
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-t", "x86_64", "--platform", "26",
        "-o", rustJniLibs.get().asFile.absolutePath,
        "build", "--release", "--lib",
    )
}

val generateUniffiBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates the Kotlin bindings for the Rust core"
    dependsOn(buildRustCore)
    workingDir(coreDir)
    coreInputs.forEach { inputs.files(coreDir.file(it).asFile) }
    outputs.dir(uniffiKotlin)
    commandLine(
        "cargo", "run", "-q", "--features", "cli", "--bin", "uniffi-bindgen", "--",
        "generate", "--no-format", "--language", "kotlin",
        "--library", cargoTarget.resolve("aarch64-linux-android/release/libccez_keys_core.so").absolutePath,
        "--out-dir", uniffiKotlin.get().asFile.absolutePath,
    )
}

android {
    namespace = "uk.ccez.keys"
    compileSdk = 37

    defaultConfig {
        applicationId = "uk.ccez.keys"
        minSdk = 26
        targetSdk = 36
        versionCode = (System.getenv("CCEZ_KEYS_VERSION_CODE") ?: "1").toInt()
        versionName = System.getenv("CCEZ_KEYS_VERSION_NAME") ?: "0.1.0-dev"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    signingConfigs {
        // One key for every build (scripts/build.sh points at it), so each
        // APK installs over the last; without it, the debug key.
        val keystore = System.getenv("CCEZ_KEYS_KEYSTORE")?.let { file(it) }
        if (keystore != null && keystore.exists()) {
            create("release") {
                storeFile = keystore
                storePassword = System.getenv("CCEZ_KEYS_KEY_PASSWORD")
                keyAlias = "ccez-keys"
                keyPassword = System.getenv("CCEZ_KEYS_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets.getByName("main") {
        jniLibs.directories.add(rustJniLibs.get().asFile.absolutePath)
        kotlin.directories.add(uniffiKotlin.get().asFile.absolutePath)
        // Word lists and faces (data/), read at run time.
        assets.directories.add(dataDir.asFile.absolutePath)
    }

    packaging { resources.excludes += "/META-INF/{AL2.0,LGPL2.1}" }
}

kotlin { jvmToolchain(21) }

tasks.named("preBuild") { dependsOn(generateUniffiBindings) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.androidx.savedstate)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.kotlinx.coroutines.android)
    implementation("${libs.jna.get()}@aar")
    testImplementation(libs.junit)
}
