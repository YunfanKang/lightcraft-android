//! The activity: the entry point `android_main`, the eframe app around [`LightcraftApp`], and the
//! services the shared UI gets on a tablet (the in-app file browser, the settings file).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Sender, channel};

use lightcraft_engine::Session;
use lightcraft_ui_egui::panels::library_problem::LibraryProblem;
use lightcraft_ui_egui::pick::PickRequest;
use lightcraft_ui_egui::{LightcraftApp, Services};
use winit::platform::android::activity::AndroidApp;

use crate::browser::{Browser, Done};
use crate::dirs::{self, Dirs};
use crate::prefs;

/// File dialogs the UI asked for, answered by the browser window one at a time.
type Requests = Rc<RefCell<VecDeque<(PickRequest, Sender<Vec<String>>)>>>;

struct Host {
    app: LightcraftApp,
    prefs: prefs::Saver,
    requests: Requests,
    browser: Option<(Browser, Sender<Vec<String>>)>,
    places: Vec<(String, PathBuf)>,
    /// Where the next dialog opens: the last folder used.
    last_dir: PathBuf,
    /// The interface zoom was set for the screen (once; after that the user's).
    zoomed: bool,
}

/// How long quitting waits for renders that are running.
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The persistent library at `dir` (created and seeded with the demo photos the first time). If it
/// can't be opened the session is empty and in memory, and the problem is shown.
fn open_session(dir: &std::path::Path) -> (Session, Option<LibraryProblem>) {
    let fresh = || Session::new().with_fs().with_system_clock();
    let mut s = fresh();
    match std::fs::create_dir_all(dir)
        .map_err(|e| e.to_string())
        .and_then(|()| s.open_library(dir, true).map(|r| (r.replayed, r.torn_bytes)).map_err(|e| e.to_string()))
    {
        Ok((replayed, torn)) => {
            log::info!(
                "library {}: {} photos, {replayed} log records replayed{}",
                dir.display(),
                s.catalog.len(),
                if torn > 0 { ", torn tail repaired" } else { "" }
            );
            (s, None)
        }
        Err(e) => {
            log::error!("can't open library {}: {e}", dir.display());
            (fresh(), Some(LibraryProblem::new(dir.to_string_lossy(), e)))
        }
    }
}

impl Host {
    fn new(cc: &eframe::CreationContext<'_>, dirs: Dirs) -> Host {
        let prefs_path = dirs.config.join("ui.json");
        let loaded = prefs::load(&prefs_path, now_secs());
        let first_run = loaded.ui.is_none() && loaded.notice.is_none();
        let (session, problem) = open_session(&dirs.library);
        let requests: Requests = Rc::new(RefCell::new(VecDeque::new()));
        let queue = requests.clone();
        let repaint = cc.egui_ctx.clone();
        let services = Services {
            picker: Some(Box::new(move |req: PickRequest| {
                let (tx, rx) = channel();
                queue.borrow_mut().push_back((req, tx));
                repaint.request_repaint();
                Ok(rx)
            })),
            ..Services::default()
        };
        let mut app = LightcraftApp::new(session, services);
        if let Some(ui) = loaded.ui {
            app.ui = ui;
        }
        if first_run {
            // the camera roll and the usual photo folders in the sidebar's Local section
            app.ui.local_roots = dirs::photo_folders(&dirs).into_iter().filter(|p| p.is_dir()).map(|p| p.to_string_lossy().into_owned()).collect();
        }
        app.ui.settings.library_path = dirs.library.to_string_lossy().into_owned();
        if app.session.last_export.is_none() {
            // Export's default folder is in the user's Pictures on the desktop; here beside the library
            app.session.last_export = Some(serde_json::json!({"dir": dirs.export.to_string_lossy()}));
        }
        lightcraft_ui_egui::i18n::set_language(app.ui.language);
        app.notices.extend(loaded.notice);
        app.library_problem = problem;
        app.gpu_notice = true;
        let written = if loaded.keep_file { Vec::new() } else { serde_json::to_vec_pretty(&app.ui).unwrap_or_default() };
        let prefs = prefs::Saver::new((!loaded.keep_file).then_some(prefs_path), written);
        let places = dirs::places(&dirs);
        let last_dir = places.iter().skip(2).map(|(_, p)| p.clone()).find(|p| p.is_dir()).unwrap_or_else(|| dirs.library.clone());
        Host { app, prefs, requests, browser: None, places, last_dir, zoomed: false }
    }
}

impl eframe::App for Host {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.zoomed {
            self.zoomed = true;
            let size = ctx.content_rect().size();
            let zoom = dirs::tablet_zoom(size.x, size.y);
            log::info!("screen {}×{} points: interface zoom {zoom}", size.x, size.y);
            ctx.set_zoom_factor(zoom);
        }
        if self.browser.is_none()
            && let Some((req, reply)) = self.requests.borrow_mut().pop_front()
        {
            let start = if self.last_dir.is_dir() { self.last_dir.clone() } else { self.places.first().map(|(_, p)| p.clone()).unwrap_or_default() };
            self.browser = Some((Browser::open(req, start, self.places.clone()), reply));
        }
        self.app.logic(ctx);
        let now = ctx.input(|i| i.time);
        if let Some(e) = self.prefs.tick(&self.app.ui, now) {
            self.app.toast_error(ctx, e);
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.perf.host_frame_ms = frame.info().cpu_usage.map(|s| f64::from(s) * 1000.0);
        self.app.ui(ui);
        if let Some((browser, _)) = self.browser.as_mut()
            && let Some(done) = browser.show(ui.ctx())
        {
            let Some((browser, reply)) = self.browser.take() else { return };
            self.last_dir = browser.dir().to_path_buf();
            let paths = match done {
                Done::Picked(paths) => paths,
                Done::Cancelled => Vec::new(),
            };
            // the command's dialog went away with the command: nobody to tell
            let _ = reply.send(paths);
            ui.ctx().request_repaint();
        }
    }

    fn on_exit(&mut self) {
        if let Err(e) = self.prefs.save(&self.app.ui) {
            log::error!("{e}");
        }
        if let Err(e) = self.app.session.close_library() {
            log::error!("saving the library failed: {e}");
        }
        if !self.app.shutdown(EXIT_WAIT) {
            log::warn!("quit: background work was still running after {EXIT_WAIT:?}");
        }
    }
}

/// The activity's entry point (`android-activity` calls it on its own thread once the
/// `NativeActivity` is created). Returning ends the activity; a failure is logged, never a panic.
// The symbol name is what `android-activity` resolves (`extern "Rust" { fn android_main(..) }`),
// so it must not be mangled; the lint counts the attribute as unsafe code, not any block here.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("lightcraft"));
    let Some(internal) = app.internal_data_path() else {
        log::error!("no internal data path: LightCraft can't keep a library or settings here");
        return;
    };
    let dirs = Dirs::of(&internal, app.external_data_path().as_deref());
    log::info!("settings {}, library {}, shared storage {:?}", dirs.config.display(), dirs.library.display(), dirs.shared);
    if let Err(e) = std::fs::create_dir_all(&dirs.config) {
        log::warn!("settings folder {}: {e}; settings are not saved", dirs.config.display());
    }
    // `std::env::set_var` is `unsafe` since Rust 2024 and this crate denies `unsafe`, so the folder
    // goes to the engine directly (`$LIGHTCRAFT_CONFIG_DIR`, if an agent set one, still wins). The
    // activity can be created again in the same process: the folder is the same then.
    if let Err(first) = lightcraft_engine::config::set_host_config_dir(dirs.config.clone())
        && first != dirs.config
    {
        log::warn!("settings folder stays {}", first.display());
    }
    let options = eframe::NativeOptions {
        android_app: Some(app),
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("LightCraft"),
        ..Default::default()
    };
    let result = eframe::run_native("LightCraft", options, Box::new(move |cc| Ok(Box::new(Host::new(cc, dirs)))));
    if let Err(e) = result {
        log::error!("{}", crate::start_failure(&e));
    }
}
