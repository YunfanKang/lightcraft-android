//! LightCraft on Android: the same egui UI as the desktop app (`crates/ui-egui`), hosted in a
//! `NativeActivity` through eframe's winit backend and drawn with wgpu (Vulkan). A tablet in
//! landscape fits the desktop layout; the host adds what a tablet lacks around it:
//! - a persistent library and settings in the app's own storage ([`dirs`], [`prefs`]);
//! - a file browser in place of the native dialogs, for Import Photos…, Export's folder and the
//!   preset files ([`browser`]); shared storage (the camera roll) needs the photos permission;
//! - finger-sized controls (an interface zoom chosen for the screen) and the soft keyboard
//!   (winit shows it for text fields);
//! - the `log` crate on logcat (tag `lightcraft`).
//!
//! `cargo xtask android` checks that the crate compiles for `aarch64-linux-android`;
//! `cargo xtask apk` builds the APK (docs/android.md). What is still missing (a share sheet,
//! MediaStore export, the permission prompt itself, a phone layout) is listed there.
// `deny`, not `forbid`: the activity's entry point must be exported under its own name
// (`#[unsafe(no_mangle)]`, which the `unsafe_code` lint counts), and it is allowed on that one
// item alone. There is no `unsafe` block in this crate (AGENTS.md / CLAUDE.md → Never crash).
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod browser;
pub mod dirs;
pub mod prefs;

#[cfg(target_os = "android")]
mod host;

/// The one-line reason the window can't open, for logcat.
pub fn start_failure(e: &dyn std::fmt::Display) -> String {
    format!("LightCraft could not open its window: {e}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn start_failure_names_the_cause() {
        assert_eq!(super::start_failure(&"no Vulkan device"), "LightCraft could not open its window: no Vulkan device");
    }
}
