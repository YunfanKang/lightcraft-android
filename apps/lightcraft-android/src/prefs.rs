//! The app settings file (`<config>/ui.json`): the desktop's format, so a settings file is the
//! same thing on every host. Loaded at start (a damaged file is set aside, never overwritten
//! silently), written when it changed (every few seconds, and at exit), atomically.

use std::path::{Path, PathBuf};

use lightcraft_ui_egui::UiState;

/// What loading the settings file found.
pub struct Loaded {
    pub ui: Option<UiState>,
    /// What to tell the user, if anything.
    pub notice: Option<String>,
    /// Don't write the file this session (it couldn't be read, or set aside).
    pub keep_file: bool,
}

/// Decode a saved settings file. A docking layout that doesn't validate is dropped (the other
/// settings are kept; `true`), as on the desktop.
fn decode(bytes: &[u8]) -> Result<(UiState, bool), String> {
    let mut saved: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let fields = saved.as_object_mut().ok_or("settings must be an object")?;
    let recovered =
        fields.get("docking").is_some_and(|v| match serde_json::from_value::<Option<lightcraft_ui_egui::docking::Workspace>>(v.clone()) {
            Ok(Some(w)) => w.validate().is_err(),
            Ok(None) => false,
            Err(_) => true,
        });
    if recovered {
        fields.remove("docking");
    }
    let ui: UiState = serde_json::from_value(saved).map_err(|e| e.to_string())?;
    Ok((ui, recovered))
}

/// Read the settings at `path`. `now_secs` names the copy a damaged file is kept as.
pub fn load(path: &Path, now_secs: u64) -> Loaded {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded { ui: None, notice: None, keep_file: false },
        Err(e) => {
            let notice = format!("The app settings ({}) couldn't be read: {e}. Defaults are used, and the file isn't overwritten.", path.display());
            return Loaded { ui: None, notice: Some(notice), keep_file: true };
        }
    };
    match decode(&bytes) {
        Ok((ui, false)) => Loaded { ui: Some(ui.sanitized()), notice: None, keep_file: false },
        Ok((ui, true)) => {
            let notice = format!(
                "The saved panel layout in {} couldn't be restored. Other settings were loaded; the file is preserved and settings won't be saved this session.",
                path.display()
            );
            Loaded { ui: Some(ui.sanitized()), notice: Some(notice), keep_file: true }
        }
        Err(e) => {
            let keep = path.with_file_name(format!("ui.json.corrupt-{now_secs}"));
            let kept = std::fs::rename(path, &keep);
            let what = match &kept {
                Ok(()) => format!("it was kept as {}", keep.display()),
                Err(r) => format!("it couldn't be set aside ({r})"),
            };
            let notice = format!("The app settings ({}) are damaged ({e}); {what}. Defaults are used.", path.display());
            Loaded { ui: None, notice: Some(notice), keep_file: kept.is_err() }
        }
    }
}

/// Write `bytes` to `path` through a temp file renamed over it: a crash or a full disk leaves the
/// old file or the new one, never a truncated one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)
}

/// Writes the settings when they changed.
pub struct Saver {
    path: Option<PathBuf>,
    /// What's in the file now, to write only changes.
    written: Vec<u8>,
    /// When the settings were last compared with the file (interface seconds).
    checked: f64,
    /// The last write failed: reported once, until a write works.
    failing: bool,
}

/// How often changed settings are written.
const EVERY_SECS: f64 = 3.0;

impl Saver {
    /// `path` `None` writes nothing. `written` is the file's content at start, if it was loaded.
    pub fn new(path: Option<PathBuf>, written: Vec<u8>) -> Saver {
        // (the first tick writes: interface time starts at zero)
        Saver { path, written, checked: -EVERY_SECS, failing: false }
    }

    /// Write `ui` if it changed since the last write.
    pub fn save(&mut self, ui: &UiState) -> Result<(), String> {
        let Some(path) = self.path.clone() else { return Ok(()) };
        let bytes = serde_json::to_vec_pretty(ui).map_err(|e| e.to_string())?;
        if bytes == self.written {
            return Ok(());
        }
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| write_atomic(&path, &bytes))
            .map_err(|e| format!("saving the app settings failed: {e}"))?;
        self.written = bytes;
        Ok(())
    }

    /// Every few seconds: save changes. A failure is returned once (to show), then again only
    /// after a write worked.
    pub fn tick(&mut self, ui: &UiState, now: f64) -> Option<String> {
        if now - self.checked < EVERY_SECS {
            return None;
        }
        self.checked = now;
        match self.save(ui) {
            Ok(()) => {
                self.failing = false;
                None
            }
            Err(e) => {
                let first = !self.failing;
                self.failing = true;
                first.then_some(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lc-android-prefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn settings_round_trip_and_only_changes_are_written() {
        let dir = temp("roundtrip");
        let path = dir.join("ui.json");
        let mut ui = UiState::default();
        ui.settings.library_path = "/storage/emulated/0/Android/data/x/files/Library".into();
        let mut saver = Saver::new(Some(path.clone()), Vec::new());
        assert_eq!(saver.tick(&ui, 0.0), None, "the first tick writes");
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        let loaded = load(&path, 1);
        assert_eq!(loaded.ui.map(|u| u.settings.library_path), Some(ui.settings.library_path.clone()));
        assert!(loaded.notice.is_none() && !loaded.keep_file);
        assert_eq!(saver.tick(&ui, 1.0), None, "too soon: no write");
        assert_eq!(saver.tick(&ui, 10.0), None, "nothing changed: no write");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), first);
        assert!(!dir.join("ui.json.tmp").exists(), "the temp file is renamed away");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_set_aside_and_defaults_are_used() {
        let dir = temp("damaged");
        let path = dir.join("ui.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let loaded = load(&path, 42);
        assert!(loaded.ui.is_none());
        assert!(!loaded.keep_file, "set aside: the next save may write");
        let notice = loaded.notice.unwrap();
        assert!(notice.contains("ui.json.corrupt-42"), "{notice}");
        assert!(dir.join("ui.json.corrupt-42").is_file() && !path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_a_first_run() {
        let loaded = load(Path::new("/nonexistent/lightcraft/ui.json"), 0);
        assert!(loaded.ui.is_none() && loaded.notice.is_none() && !loaded.keep_file);
    }

    #[test]
    fn a_write_failure_is_reported_once() {
        let mut saver = Saver::new(Some(PathBuf::from("/proc/no-such-dir/ui.json")), Vec::new());
        let ui = UiState::default();
        assert!(saver.tick(&ui, 0.0).is_some_and(|e| e.contains("saving the app settings failed")));
        assert_eq!(saver.tick(&ui, 10.0), None, "still failing: not repeated");
        assert_eq!(Saver::new(None, Vec::new()).tick(&ui, 0.0), None, "no path: nothing to do");
    }
}
