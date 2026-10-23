# LightCraft on Android

`apps/lightcraft-android` hosts the same egui UI as the desktop app (`crates/ui-egui`) in an
Android `NativeActivity`, through eframe's winit backend, drawn with wgpu on Vulkan. Everything
below the window (engine, raw decoders, codecs, pipeline, catalog) is the desktop's code, unchanged:
the WASM build already proved it has no operating-system assumptions, and Android keeps native
threads (rayon), so there is no worker layer like the web host's.

**Status: a skeleton, not a product.** It builds, opens the app on the procedural demo library and
logs to logcat. See [Status](#status) for what is missing before it is usable.

## Check it compiles

```sh
rustup target add aarch64-linux-android
cargo xtask android        # cargo check --target aarch64-linux-android -p lightcraft-android --lib
```

This is part of `cargo xtask ci`. A check links nothing, so no NDK is needed: it catches
`cfg(target_os)` gaps, dependencies that do not build for Android, and uses of APIs the platform
lacks, on any machine. The target is 64-bit ARM only (every phone and tablet since 2019; the one
ABI Google Play accepts on its own).

## Build an APK

The repository does not ship Gradle files yet. The crate is a `cdylib`, so any of the usual
Rust-on-Android routes packages it:

```sh
cargo install cargo-ndk
export ANDROID_NDK_HOME=<Android SDK>/ndk/<version>
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build --release -p lightcraft-android
```

then an Android Studio project whose activity is `android.app.NativeActivity` with
`<meta-data android:name="android.app.lib_name" android:value="lightcraft_android" />` in its
manifest, minimum SDK 26 (Vulkan 1.1 is widespread from there), and the `.so` under
`jniLibs/arm64-v8a/`. `cargo apk` (`cargo install cargo-apk`) does the same without Gradle for a
quick device test. Both need the NDK (its `clang` is the linker); the pure-Rust rule holds because
nothing of ours is C: the NDK only links.

## How the host differs from the desktop

| | Desktop (`apps/lightcraft`) | Android (`apps/lightcraft-android`) |
|---|---|---|
| Entry | `fn main`, `eframe::run_native` | `android_main(AndroidApp)` (called by `android-activity` on its own thread), `eframe::run_native` with `NativeOptions::android_app` |
| Window backend | winit + Wayland/X11/AppKit/Win32 | winit's `android-native-activity` (pure Rust; the GameActivity backend compiles C++ and is not used) |
| Accessibility | AccessKit | none: AccessKit has no NativeActivity adapter, and eframe rejects `accesskit` with it |
| Settings folder | `~/.config/lightcraft` etc. (`engine::config`) | the activity's private data directory, handed to `lightcraft_engine::config::set_host_config_dir` (Android has no `$HOME`; `$LIGHTCRAFT_CONFIG_DIR` still wins) |
| Library | the user's folders on disk | the demo library, in memory, for now |
| File dialogs | `rfd` | none yet (`Services::default()`) |
| Logging | file + stderr (`logging.rs`) | logcat, tag `lightcraft` (`android_logger`) |

## Status

What has to land before the build is usable, roughly in order:

1. **Import from the device.** `rfd` has no Android backend; photos arrive through the Storage
   Access Framework (`ACTION_OPEN_DOCUMENT`) or MediaStore, which means JNI calls into the activity
   (the `jni` crate is pure Rust). Imported originals should be copied into app storage by content
   hash, as `apps/lightcraft-web` does, since a content URI is not a path.
2. **A persistent library** in the private data directory (`FsStore` over `internal_data_path`),
   and export to shared storage (MediaStore insert) instead of `Services::write`.
3. **A compact layout.** `crates/ui-egui` assumes `window_place::MIN_SIZE` (900 × 560 points) with
   side panels, filmstrip and menu bar; a phone in portrait is about 400 points wide. Pinch zoom and
   multi-touch already work in the loupe. Tablets in landscape fit the desktop layout nearly as is
   and are the sensible first target.
4. **Memory.** Phones give an app one to two gigabytes; the pipeline's draft and tile paths help, the
   `denoise` and `sam` features stay off, and `lightcraft-sysmem` is a no-op off macOS.
5. **Gradle project and a release job** (`release.yml`), signing, and a Play listing.

A row in [`docs/parity-checklist.md`](parity-checklist.md) is not opened for Android until step 2:
nothing a user can do on the phone is a Lightroom feature yet.
