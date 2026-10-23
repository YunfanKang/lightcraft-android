//! The file browser that stands in for the native file dialogs (`rfd` has no Android backend):
//! an egui window over the app that answers a [`PickRequest`] with paths, for the commands that
//! ask through `Services::picker`. Rows are finger-sized. Listing is a pure function; the window
//! shows why a folder can't be read (shared storage needs the photos permission).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lightcraft_ui_egui::i18n::tr;
use lightcraft_ui_egui::pick::{PickKind, PickRequest};
use lightcraft_ui_egui::text_field::TextField;

/// One row of a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Whether a file name has one of `extensions` (case-insensitive; `None` admits every file).
pub fn admits(name: &str, extensions: Option<&[&str]>) -> bool {
    let Some(exts) = extensions else { return true };
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// The folders, then the admitted files of `dir`, each sorted by name (case-insensitive); hidden
/// entries (`.name`) left out.
pub fn list(dir: &Path, extensions: Option<&[&str]>) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| read_error(dir, &e))?;
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        // (a symlink is followed for its kind; Android's shared storage has none anyway)
        let is_dir = if kind.is_symlink() { entry.path().is_dir() } else { kind.is_dir() };
        if is_dir {
            dirs.push(Entry { name, path: entry.path(), is_dir: true });
        } else if admits(&name, extensions) {
            files.push(Entry { name, path: entry.path(), is_dir: false });
        }
    }
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name));
    dirs.sort_by(by_name);
    files.sort_by(by_name);
    dirs.append(&mut files);
    Ok(dirs)
}

/// Why `dir` can't be listed, with what to do about it.
pub fn read_error(dir: &Path, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "LightCraft can't read {}. Allow Photos and videos (or Files and media) for LightCraft in Android Settings ▸ Apps ▸ LightCraft ▸ Permissions, then come back.",
            dir.display()
        )
    } else {
        format!("{}: {e}", dir.display())
    }
}

/// The dialog's title when the request names none.
pub fn default_title(kind: PickKind) -> &'static str {
    match kind {
        PickKind::Files => "Choose Photos",
        PickKind::File => "Choose a File",
        PickKind::Folder => "Choose a Folder",
        PickKind::Save => "Save As",
    }
}

/// A file dialog that is up.
pub struct Browser {
    request: PickRequest,
    dir: PathBuf,
    entries: Result<Vec<Entry>, String>,
    selected: BTreeSet<PathBuf>,
    /// [`PickKind::Save`]: the file name.
    file_name: String,
    places: Vec<(String, PathBuf)>,
}

/// How the dialog ended.
pub enum Done {
    /// The chosen paths (a save: the one file to write).
    Picked(Vec<String>),
    Cancelled,
}

impl Browser {
    /// A dialog for `request`, opened at `start` (the last folder used, or a place).
    pub fn open(request: PickRequest, start: PathBuf, places: Vec<(String, PathBuf)>) -> Browser {
        let file_name = request.file_name.clone().unwrap_or_default();
        let mut b = Browser { request, dir: PathBuf::new(), entries: Ok(Vec::new()), selected: BTreeSet::new(), file_name, places };
        b.go(start);
        b
    }

    /// The folder shown.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn extensions(&self) -> Option<&'static [&'static str]> {
        match self.request.kind {
            PickKind::Folder => Some(&[]),
            _ => self.request.filter.as_ref().map(|(_, exts)| *exts),
        }
    }

    fn go(&mut self, dir: PathBuf) {
        self.entries = list(&dir, self.extensions());
        self.dir = dir;
        self.selected.clear();
    }

    fn chosen(&self) -> Vec<String> {
        self.selected.iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }

    /// Draw the dialog; `Some` when it closed.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Done> {
        let title = self.request.title.clone().unwrap_or_else(|| tr(default_title(self.request.kind)).to_string());
        let mut done = None;
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new(title)
            .id(egui::Id::new("android.browser"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2((screen.width() * 0.8).min(900.0), (screen.height() * 0.85).min(700.0)))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 36.0;
                ui.spacing_mut().button_padding = egui::vec2(12.0, 8.0);
                ui.horizontal_wrapped(|ui| {
                    for (name, path) in self.places.clone() {
                        if path.is_dir() && ui.selectable_label(self.dir == path, tr(&name)).clicked() {
                            self.go(path);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let up = self.dir.parent().map(Path::to_path_buf);
                    if ui.add_enabled(up.is_some(), egui::Button::new("⬆")).clicked()
                        && let Some(up) = up
                    {
                        self.go(up);
                    }
                    ui.label(self.dir.to_string_lossy());
                });
                ui.separator();
                let list_height = ui.available_height() - 56.0;
                egui::ScrollArea::vertical().max_height(list_height.max(120.0)).auto_shrink([false, true]).show(ui, |ui| match &self.entries {
                    Err(e) => {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                    Ok(entries) => {
                        if entries.is_empty() {
                            ui.weak(tr("Nothing here"));
                        }
                        let mut go = None;
                        for e in entries.clone() {
                            if e.is_dir {
                                if ui.add(egui::Button::new(format!("🗀  {}", e.name)).frame(false)).clicked() {
                                    go = Some(e.path);
                                }
                            } else {
                                let on = self.selected.contains(&e.path);
                                if ui.selectable_label(on, &e.name).clicked() {
                                    match self.request.kind {
                                        PickKind::Files => {
                                            if on {
                                                self.selected.remove(&e.path);
                                            } else {
                                                self.selected.insert(e.path);
                                            }
                                        }
                                        PickKind::Save => self.file_name = e.name,
                                        _ => {
                                            self.selected.clear();
                                            self.selected.insert(e.path);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(dir) = go {
                            self.go(dir);
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("Cancel")).clicked() {
                        done = Some(Done::Cancelled);
                    }
                    match self.request.kind {
                        PickKind::Files | PickKind::File => {
                            let n = self.selected.len();
                            let label = if n > 1 { format!("{} ({n})", tr("Choose")) } else { tr("Choose").to_string() };
                            if ui.add_enabled(n > 0, egui::Button::new(label)).clicked() {
                                done = Some(Done::Picked(self.chosen()));
                            }
                        }
                        PickKind::Folder => {
                            if ui.add_enabled(self.entries.is_ok(), egui::Button::new(tr("Choose This Folder"))).clicked() {
                                done = Some(Done::Picked(vec![self.dir.to_string_lossy().into_owned()]));
                            }
                        }
                        PickKind::Save => {
                            TextField::singleline("android.browser.name", &mut self.file_name).width(260.0).show(ui);
                            let name = self.file_name.trim();
                            let ok = !name.is_empty() && !name.contains('/') && self.entries.is_ok();
                            if ui.add_enabled(ok, egui::Button::new(tr("Save"))).clicked() {
                                done = Some(Done::Picked(vec![self.dir.join(name).to_string_lossy().into_owned()]));
                            }
                        }
                    }
                });
            });
        if !open {
            done = Some(Done::Cancelled);
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lc-android-browser-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn extensions_match_case_insensitively() {
        let photos: &[&str] = &["jpg", "arw"];
        assert!(admits("IMG_0001.JPG", Some(photos)));
        assert!(admits("a.b.arw", Some(photos)));
        assert!(!admits("notes.txt", Some(photos)));
        assert!(!admits("noext", Some(photos)));
        assert!(admits("anything", None));
        assert!(!admits("file.jpg", Some(&[])), "a folder pick lists no files");
    }

    #[test]
    fn folders_come_first_then_admitted_files_sorted_hidden_left_out() {
        let dir = temp("list");
        std::fs::create_dir(dir.join("b-folder")).unwrap();
        std::fs::create_dir(dir.join("A-folder")).unwrap();
        std::fs::create_dir(dir.join(".thumbnails")).unwrap();
        for f in ["zeta.JPG", "alpha.jpg", "notes.txt", ".hidden.jpg"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let names: Vec<String> =
            list(&dir, Some(&["jpg"])).unwrap().into_iter().map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" })).collect();
        assert_eq!(names, ["A-folder/", "b-folder/", "alpha.jpg", "zeta.JPG"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_cannot_be_read_says_so() {
        let e = list(Path::new("/nonexistent/DCIM"), None).unwrap_err();
        assert!(e.starts_with("/nonexistent/DCIM: "), "{e}");
        let denied = read_error(Path::new("/storage/emulated/0/DCIM"), &std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(denied.contains("Photos and videos") && denied.contains("/storage/emulated/0/DCIM"), "{denied}");
    }

    #[test]
    fn a_dialog_opens_at_its_start_folder_and_lists_it() {
        let dir = temp("open");
        std::fs::write(dir.join("one.png"), b"").unwrap();
        let req = PickRequest::files(None, "Photos", &["png"]);
        let b = Browser::open(req, dir.clone(), Vec::new());
        assert_eq!(b.dir(), dir);
        assert_eq!(b.entries.as_ref().unwrap().len(), 1);
        assert_eq!(default_title(PickKind::Files), "Choose Photos");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
