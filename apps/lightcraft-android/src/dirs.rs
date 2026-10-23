//! Where LightCraft keeps its data on the device, the folders the user can browse, and the
//! interface zoom for the screen. Pure: the host feeds in what the activity reports.

use std::path::{Path, PathBuf};

use lightcraft_ui_egui::window_place::MIN_SIZE;

/// This install's folders. All in the app's own storage, which needs no permission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// Settings (`ui.json`), camera profiles, presets: the activity's private data directory.
    pub config: PathBuf,
    /// The library (catalog journal, `Originals/`, thumbnails). In the app's external storage when
    /// the device has it (`Android/data/<package>/files/Library`, visible over USB), else internal.
    pub library: PathBuf,
    /// The default export folder, beside the library.
    pub export: PathBuf,
    /// Shared storage (`/storage/emulated/0`), where DCIM, Pictures and Download are. Reading it
    /// needs the photos permission (granted in Android's app settings; see docs/android.md).
    pub shared: Option<PathBuf>,
}

impl Dirs {
    /// From the activity's `internal_data_path` and `external_data_path`.
    pub fn of(internal: &Path, external: Option<&Path>) -> Dirs {
        let data = external.unwrap_or(internal);
        Dirs {
            config: internal.join("lightcraft"),
            library: data.join("Library"),
            export: data.join("Export"),
            shared: external.and_then(shared_storage),
        }
    }
}

/// `/storage/emulated/0` from `/storage/emulated/0/Android/data/<package>/files`: the folder whose
/// child is `Android`.
pub fn shared_storage(external: &Path) -> Option<PathBuf> {
    let mut dir = external;
    while let Some(parent) = dir.parent() {
        if dir.file_name().is_some_and(|n| n == "Android") {
            return Some(parent.to_path_buf());
        }
        dir = parent;
    }
    None
}

/// The places the file browser offers, in order (the app's own folders first, then shared
/// storage). The browser shows those that exist.
pub fn places(dirs: &Dirs) -> Vec<(String, PathBuf)> {
    let mut v = vec![("Library".to_string(), dirs.library.clone()), ("Exports".to_string(), dirs.export.clone())];
    if let Some(s) = &dirs.shared {
        for (name, sub) in [("Camera", "DCIM"), ("Pictures", "Pictures"), ("Downloads", "Download")] {
            v.push((name.to_string(), s.join(sub)));
        }
        v.push(("Storage".to_string(), s.clone()));
    }
    v
}

/// The shared-storage folders photos usually are in, for the sidebar's Local section on a first
/// run (the user can hide or add to them as on the desktop).
pub fn photo_folders(dirs: &Dirs) -> Vec<PathBuf> {
    dirs.shared.iter().flat_map(|s| ["DCIM", "Pictures", "Download"].map(|sub| s.join(sub))).collect()
}

/// The interface zoom for a screen of `width` × `height` points: bigger controls for fingers, as
/// long as the desktop layout (`MIN_SIZE`) still fits.
pub fn tablet_zoom(width: f32, height: f32) -> f32 {
    let [min_w, min_h] = MIN_SIZE;
    [1.25, 1.1].into_iter().find(|z| width / z >= min_w && height / z >= min_h).unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERNAL: &str = "/data/user/0/ai.storyteller.lightcraft/files";
    const EXTERNAL: &str = "/storage/emulated/0/Android/data/ai.storyteller.lightcraft/files";

    #[test]
    fn library_and_exports_go_to_external_storage_when_there_is_one() {
        let d = Dirs::of(Path::new(INTERNAL), Some(Path::new(EXTERNAL)));
        assert_eq!(d.config, Path::new("/data/user/0/ai.storyteller.lightcraft/files/lightcraft"), "settings stay internal");
        assert_eq!(d.library, Path::new(&format!("{EXTERNAL}/Library")));
        assert_eq!(d.export, Path::new(&format!("{EXTERNAL}/Export")));
        assert_eq!(d.shared.as_deref(), Some(Path::new("/storage/emulated/0")));
    }

    #[test]
    fn without_external_storage_everything_is_internal_and_nothing_is_shared() {
        let d = Dirs::of(Path::new(INTERNAL), None);
        assert_eq!(d.library, Path::new(&format!("{INTERNAL}/Library")));
        assert_eq!(d.shared, None);
        assert_eq!(places(&d).len(), 2, "only the app's own folders");
        assert!(photo_folders(&d).is_empty());
    }

    #[test]
    fn shared_storage_is_the_parent_of_android() {
        assert_eq!(shared_storage(Path::new(EXTERNAL)).as_deref(), Some(Path::new("/storage/emulated/0")));
        assert_eq!(shared_storage(Path::new("/mnt/expand/abc/user/0/pkg/files")), None, "no Android folder above it");
    }

    #[test]
    fn places_list_the_camera_roll_after_the_apps_folders() {
        let d = Dirs::of(Path::new(INTERNAL), Some(Path::new(EXTERNAL)));
        let p = places(&d);
        let names: Vec<&str> = p.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Library", "Exports", "Camera", "Pictures", "Downloads", "Storage"]);
        assert_eq!(p[2].1, Path::new("/storage/emulated/0/DCIM"));
        assert_eq!(
            photo_folders(&d),
            [Path::new("/storage/emulated/0/DCIM"), Path::new("/storage/emulated/0/Pictures"), Path::new("/storage/emulated/0/Download")]
        );
    }

    #[test]
    fn zoom_grows_controls_while_the_layout_fits() {
        assert_eq!(tablet_zoom(1280.0, 800.0), 1.25, "a 10-inch tablet at 1.5× density");
        assert_eq!(tablet_zoom(1024.0, 640.0), 1.1, "1.25 would drop under the minimum width");
        assert_eq!(tablet_zoom(960.0, 600.0), 1.0, "barely the minimum: no zoom");
        assert_eq!(tablet_zoom(800.0, 1280.0), 1.0, "portrait: the layout doesn't fit anyway");
    }
}
