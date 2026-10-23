//! LightCraft on Android: the same egui UI as the desktop app (`crates/ui-egui`), hosted in a
//! `NativeActivity` through eframe's winit backend and drawn with wgpu (Vulkan).
//!
//! This is the host shell only. What it does today:
//! - opens the app on the procedural demo library, with LightCraft's settings folder in the
//!   activity's private data directory (`internal_data_path`), so nothing reads `$HOME`;
//! - forwards the `log` crate to logcat (tag `lightcraft`).
//!
//! What it does not do yet (see `docs/android.md` → Status): import from the device's photo
//! library (Storage Access Framework / MediaStore, which needs JNI), export to shared storage, a
//! phone layout (the UI assumes `window_place::MIN_SIZE`), or persistence of the library between
//! runs. `cargo xtask android` checks that this crate and everything under it compiles for
//! `aarch64-linux-android`; packaging an APK is described in the doc.
// `deny`, not `forbid`: the activity's entry point must be exported under its own name
// (`#[unsafe(no_mangle)]`, which the `unsafe_code` lint counts), and it is allowed on that one
// item alone. There is no `unsafe` block in this crate (AGENTS.md / CLAUDE.md → Never crash).
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::Path;

/// LightCraft's settings folder inside the activity's private data directory.
pub fn config_dir_in(internal_data: &Path) -> std::path::PathBuf {
    internal_data.join("lightcraft")
}

/// The one-line reason the window can't open, for logcat and the activity's error state.
pub fn start_failure(e: &dyn std::fmt::Display) -> String {
    format!("LightCraft could not open its window: {e}")
}

#[cfg(target_os = "android")]
mod host {
    use lightcraft_engine::Session;
    use lightcraft_ui_egui::{LightcraftApp, Services};

    struct App(LightcraftApp);

    impl eframe::App for App {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.0.logic(ctx);
        }
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            self.0.perf.host_frame_ms = frame.info().cpu_usage.map(|s| f64::from(s) * 1000.0);
            self.0.ui(ui);
        }
        fn on_exit(&mut self) {
            if let Err(e) = self.0.session.close_library() {
                log::error!("saving the library failed: {e}");
            }
            if !self.0.shutdown(std::time::Duration::from_secs(2)) {
                log::warn!("quit: background work was still running");
            }
        }
    }

    /// The activity's entry point (`android-activity` calls it on its own thread once the
    /// `NativeActivity` is created). Returning ends the activity; a failure is logged, never a
    /// panic.
    // The symbol name is what `android-activity` resolves (`extern "Rust" { fn android_main(..) }`),
    // so it must not be mangled; the lint counts the attribute as unsafe code, not any block here.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("lightcraft"));
        if let Some(data) = app.internal_data_path() {
            let dir = crate::config_dir_in(&data);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("settings folder {}: {e}; settings are not saved", dir.display());
            } else {
                // Read by the engine when the session opens its settings; set before any session
                // exists, on the only thread of this process that has run so far.
                set_config_dir(&dir);
            }
        } else {
            log::warn!("no internal data path: settings are not saved");
        }
        let options = eframe::NativeOptions {
            android_app: Some(app),
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default().with_title("LightCraft"),
            ..Default::default()
        };
        let result = eframe::run_native(
            "LightCraft",
            options,
            Box::new(|_cc| {
                let mut app = LightcraftApp::new(Session::with_demo(), Services::default());
                lightcraft_ui_egui::i18n::set_language(app.ui.language);
                app.gpu_notice = true;
                Ok(Box::new(App(app)))
            }),
        );
        if let Err(e) = result {
            log::error!("{}", crate::start_failure(&e));
        }
    }

    /// `std::env::set_var` is `unsafe` since Rust 2024 and this crate denies `unsafe`, so the folder
    /// goes to the engine directly (`$LIGHTCRAFT_CONFIG_DIR`, if an agent set one, still wins).
    fn set_config_dir(dir: &std::path::Path) {
        if let Err(first) = lightcraft_engine::config::set_host_config_dir(dir.to_path_buf()) {
            log::warn!("settings folder already set to {}", first.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_live_under_the_private_data_dir() {
        let dir = config_dir_in(Path::new("/data/user/0/ai.storyteller.lightcraft/files"));
        assert_eq!(dir, Path::new("/data/user/0/ai.storyteller.lightcraft/files/lightcraft"));
    }

    #[test]
    fn start_failure_names_the_cause() {
        assert_eq!(start_failure(&"no Vulkan device"), "LightCraft could not open its window: no Vulkan device");
    }
}
