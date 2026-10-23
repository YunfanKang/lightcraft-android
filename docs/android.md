# LightCraft on Android

`apps/lightcraft-android` hosts the same egui UI as the desktop app (`crates/ui-egui`) in an
Android `NativeActivity`, through eframe's winit backend, drawn with wgpu on Vulkan. Everything
below the window (engine, raw decoders, codecs, pipeline, catalog) is the desktop's code, unchanged:
the WASM build already proved it has no operating-system assumptions, and Android keeps native
threads (rayon), so there is no worker layer like the web host's.

**Status: a tablet build.** Landscape tablets (a smallest width of 600 dp, the manifest's
requirement) fit the desktop layout; the host adds a persistent library, settings, a file browser
in place of the native dialogs, finger-sized controls and logcat logging. See [Status](#status)
for what is still missing. There is no phone layout.

## Check it compiles

```sh
rustup target add aarch64-linux-android
cargo xtask android        # cargo check --target aarch64-linux-android -p lightcraft-android --lib
```

This is part of `cargo xtask ci`. A check links nothing, so no NDK is needed: it catches
`cfg(target_os)` gaps, dependencies that do not build for Android, and uses of APIs the platform
lacks, on any machine. The target is 64-bit ARM only (every phone and tablet since 2019; the one
ABI Google Play accepts on its own).

## Build the APK

```sh
cargo xtask apk              # → <target>/android/lightcraft-<version>-arm64-v8a.apk
cargo xtask apk --install    # … and `adb install -r` it on the connected device
cargo xtask apk --dev        # debug profile (faster to build, slow to run)
```

It needs the Android SDK with a platform, build-tools and an NDK, and a JDK's `keytool` on `PATH`
(for the debug signing key it makes once, in `<target>/android/debug.keystore`). One-time setup
with the SDK's command-line tools:

```sh
export ANDROID_HOME=$HOME/android-sdk
sdkmanager --sdk_root=$ANDROID_HOME "platforms;android-34" "build-tools;34.0.0" "ndk;27.2.12479018"
```

`ANDROID_NDK_HOME` names an NDK elsewhere; without it the newest under `<sdk>/ndk/` is used. The
build is `cargo build --target aarch64-linux-android --release` with the NDK's clang as the linker
(nothing of ours is C: the NDK only links against the platform's `libc`, `libandroid` and
`liblog`), then the SDK's own `aapt2`, `aapt`, `zipalign` and `apksigner` around
`packaging/android/AndroidManifest.xml` and the launcher icon from `assets/app-icon/`. No Gradle,
no Java of our own. `CRAFT_FONTS_DIR` works as for every other build (Chinese and Japanese text).

**First run on the device.** The app opens on the demo library. To see the camera roll, allow
*Photos and videos* (older Android: *Files and media*) for LightCraft in Android Settings ▸ Apps ▸
LightCraft ▸ Permissions: the host declares the permission but can't show the prompt yet (that
needs JNI, below). Then File ▸ Import Photos… browses DCIM, Pictures and Download, and the
sidebar's Local section lists those folders. Without the permission Android still lists those
folders but hides every file in them (it doesn't refuse the folder), so the browser shows an empty
Camera folder with a hint saying so. "Allow limited access" (Android 14's selected photos) counts
as no permission here: only the files picked in the system sheet are visible, and that sheet
needs JNI too.

## Where things are on the device

| | Path | Needs permission |
|---|---|---|
| Settings (`ui.json`), camera profiles, presets | `/data/user/0/ai.storyteller.lightcraft/files/lightcraft/` (the activity's `internal_data_path`) | no |
| Library (catalog journal, `Originals/`, thumbnails) | `/storage/emulated/0/Android/data/ai.storyteller.lightcraft/files/Library/` (`external_data_path`; internal storage on a device without it) | no; visible over USB |
| Exports (default folder) | `…/files/Export/` beside the library | no; visible over USB |
| Camera roll, Pictures, Download | `/storage/emulated/0/DCIM` etc. | *Photos and videos* |

Uninstalling the app removes all of it (Android deletes app-specific storage), so an export is
the way to get a developed photo out, over USB or a file manager that can open `Android/data`.

## How the host differs from the desktop

| | Desktop (`apps/lightcraft`) | Android (`apps/lightcraft-android`) |
|---|---|---|
| Entry | `fn main`, `eframe::run_native` | `android_main(AndroidApp)` (called by `android-activity` on its own thread), `eframe::run_native` with `NativeOptions::android_app` |
| Window backend | winit + Wayland/X11/AppKit/Win32 | winit's `android-native-activity` (pure Rust; the GameActivity backend compiles C++ and is not used) |
| Accessibility | AccessKit | none: AccessKit has no NativeActivity adapter, and eframe rejects `accesskit` with it |
| Settings folder | `~/.config/lightcraft` etc. (`engine::config`) | the activity's private data directory, handed to `lightcraft_engine::config::set_host_config_dir` (Android has no `$HOME`; `$LIGHTCRAFT_CONFIG_DIR` still wins) |
| Library | `~/Pictures/LightCraft Library`, or `--library` | `Library/` in the app's external storage; seeded with the demo photos the first time |
| File dialogs | `rfd`, on a thread | the host's own egui window (`browser.rs`) over the app, answering `Services::picker`; it starts in the camera roll and offers Library, Exports, Camera, Pictures, Downloads and Storage |
| Export folder default | `~/Pictures/LightCraft Exports` | `Export/` beside the library (set as the session's last export folder on a first run) |
| Controls | pointer-sized | an interface zoom chosen for the screen (1.25× on a 1280 × 800-point tablet) as long as the desktop layout still fits; the soft keyboard opens for text fields (winit) |
| System bars | the window manager keeps the window clear of them | the status bar is hidden (`FULLSCREEN` window flag: devices that lay the window out edge to edge put the menu bar under the clock), and the usable area the activity reports (`content_rect`, e.g. above the soft keyboard) is what egui lays out in |
| Logging | file + stderr (`logging.rs`) | logcat, tag `lightcraft` (`android_logger`) |
| Menus, shortcuts | native menu bar (macOS), keyboard | the in-app menu bar; keyboard shortcuts work with a hardware keyboard |

## Status

What has to land before the tablet build is comfortable, roughly in order:

1. **The permission prompt, the share sheet and system-bar insets** need JNI
   (`Activity.requestPermissions`, `ACTION_SEND`, MediaStore inserts so exports show in Photos,
   `WindowInsets` so the status bar can stay visible with the interface laid out below it). The `jni` crate is pure Rust, but
   calling into the JVM takes `unsafe`, so it goes into an isolated helper crate like `sysmem`
   (`// SAFETY:` on every block) and needs a device to test on: a wrong signature aborts the JVM.
2. **Memory.** A 24 MP raw plus pipeline intermediates is heavy for a tablet; the pipeline's draft
   and tile paths help, the `denoise` and `sam` features stay off, and `lightcraft-sysmem` is a
   no-op off macOS. Measure on a device before raising defaults.
3. **A compact layout for phones.** `crates/ui-egui` assumes `window_place::MIN_SIZE` (900 × 560
   points) with side panels, filmstrip and menu bar; a phone in portrait is about 400 points wide.
   Pinch zoom and multi-touch already work in the loupe.
4. **Release packaging**: a release signing key, `release.yml`, a Play listing.

A row in [`docs/parity-checklist.md`](parity-checklist.md) is not opened for Android: the tablet
build offers the desktop's features through the desktop's commands, and the checklist counts those
once.
