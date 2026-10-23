//! `cargo xtask apk`: build `apps/lightcraft-android` for `aarch64-linux-android` and package it as
//! an APK with the Android SDK's own tools (`aapt2`, `aapt`, `zipalign`, `apksigner`), signed with
//! a debug key made on the spot. No Gradle: the manifest is `packaging/android/AndroidManifest.xml`,
//! the launcher icon comes from `assets/app-icon/`, and the only Java is in the SDK's tools.
//!
//! Needs the SDK (`ANDROID_HOME` or `ANDROID_SDK_ROOT`) with a platform, build-tools and an NDK
//! (`ANDROID_NDK_HOME`, else the newest under `<sdk>/ndk/`), and `keytool` from a JDK on `PATH`.
//! `--dev` builds the debug profile; `--install` runs `adb install -r` on the result.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{cargo, metadata, root, run};

const TARGET: &str = "aarch64-linux-android";
const ABI: &str = "arm64-v8a";
const LIB: &str = "liblightcraft_android.so";
const MIN_SDK: u32 = 26;
const TARGET_SDK: u32 = 34;
const ICON: &str = "assets/app-icon/hicolor/256x256/apps/ai.storyteller.lightcraft.png";
const KEYSTORE_PASS: &str = "android";

pub fn run_cmd(args: &[&str]) -> Result<(), String> {
    let dev = args.contains(&"--dev");
    let install = args.contains(&"--install");
    if let Some(other) = args.iter().find(|a| !matches!(**a, "--dev" | "--install")) {
        return Err(format!("apk: unknown option `{other}`"));
    }
    let sdk = sdk_root()?;
    let tools = Tools::find(&sdk)?;
    let (version, target_dir) = workspace_version_and_target()?;
    let out = target_dir.join("android");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;

    // 1. The shared object: cargo with the NDK's clang as the linker (nothing of ours is C; the
    //    NDK only links against the platform's libc, libandroid and liblog).
    let mut c = cargo();
    c.args(["build", "--target", TARGET, "-p", "lightcraft-android", "--lib"]);
    if !dev {
        c.arg("--release");
    }
    c.env("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", &tools.linker);
    run(c, &format!("cargo build --target {TARGET} -p lightcraft-android --lib{}", if dev { "" } else { " --release" }))?;
    let so = target_dir.join(TARGET).join(if dev { "debug" } else { "release" }).join(LIB);
    if !so.is_file() {
        return Err(format!("{} was not built", so.display()));
    }

    // 2. The staging folder: manifest, icon, the library under its ABI
    let staging = out.join("staging");
    let _ = std::fs::remove_dir_all(&staging);
    let lib_dir = staging.join("lib").join(ABI);
    let res_dir = staging.join("res").join("mipmap-xxxhdpi");
    for d in [&lib_dir, &res_dir] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // stripped: the release profile keeps line tables, which make the library five times the
    // size (they stay in `target/` for symbolizing a logcat trace)
    let mut c = Command::new(&tools.strip);
    c.arg("--strip-unneeded").arg("-o").arg(lib_dir.join(LIB)).arg(&so);
    run(c, "llvm-strip --strip-unneeded")?;
    copy(&root().join(ICON), &res_dir.join("ic_launcher.png"))?;
    let manifest = root().join("packaging/android/AndroidManifest.xml");

    // 3. Resources and the manifest into an APK, then the library, aligned and signed
    let compiled = staging.join("res.zip");
    let mut c = Command::new(&tools.aapt2);
    c.args(["compile", "--dir"]).arg(staging.join("res")).arg("-o").arg(&compiled);
    run(c, "aapt2 compile")?;
    let unsigned = staging.join("unsigned.apk");
    let mut c = Command::new(&tools.aapt2);
    c.arg("link")
        .arg("-o")
        .arg(&unsigned)
        .arg("--manifest")
        .arg(&manifest)
        .arg("-I")
        .arg(&tools.android_jar)
        .args(["--min-sdk-version", &MIN_SDK.to_string(), "--target-sdk-version", &TARGET_SDK.to_string()])
        .args(["--version-code", &version_code(&version).to_string(), "--version-name", &version])
        .arg(&compiled);
    run(c, "aapt2 link")?;
    let mut c = Command::new(&tools.aapt);
    c.current_dir(&staging).arg("add").arg(&unsigned).arg(format!("lib/{ABI}/{LIB}"));
    run(c, &format!("aapt add lib/{ABI}/{LIB}"))?;
    let aligned = staging.join("aligned.apk");
    let mut c = Command::new(&tools.zipalign);
    c.args(["-f", "4"]).arg(&unsigned).arg(&aligned);
    run(c, "zipalign")?;
    let keystore = out.join("debug.keystore");
    if !keystore.is_file() {
        let mut c = Command::new("keytool");
        c.args(["-genkeypair", "-keystore"])
            .arg(&keystore)
            .args(["-storepass", KEYSTORE_PASS, "-keypass", KEYSTORE_PASS, "-alias", "androiddebugkey"])
            .args(["-keyalg", "RSA", "-keysize", "2048", "-validity", "10000", "-dname", "CN=LightCraft Debug, O=LightCraft, C=US"]);
        run(c, "keytool -genkeypair (debug key)")?;
    }
    let apk = out.join(format!("lightcraft-{version}{}-{ABI}.apk", if dev { "-dev" } else { "" }));
    let mut c = Command::new(&tools.apksigner);
    c.arg("sign")
        .arg("--ks")
        .arg(&keystore)
        .args(["--ks-pass", &format!("pass:{KEYSTORE_PASS}"), "--key-pass", &format!("pass:{KEYSTORE_PASS}")])
        .arg("--out")
        .arg(&apk)
        .arg(&aligned);
    run(c, "apksigner sign")?;
    let _ = std::fs::remove_dir_all(&staging);
    let size_mb = std::fs::metadata(&apk).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);
    println!("\nAPK: {} ({size_mb:.1} MB, debug-signed; install with `adb install -r <apk>`)", apk.display());

    if install {
        let adb = sdk.join("platform-tools").join(exe("adb"));
        let mut c = Command::new(if adb.is_file() { adb } else { PathBuf::from(exe("adb")) });
        c.args(["install", "-r"]).arg(&apk);
        run(c, "adb install -r")?;
    }
    Ok(())
}

/// `ANDROID_HOME`, else `ANDROID_SDK_ROOT`.
fn sdk_root() -> Result<PathBuf, String> {
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .find_map(|v| std::env::var_os(v).filter(|p| !p.is_empty()).map(PathBuf::from))
        .filter(|p| p.is_dir())
        .ok_or_else(|| "the Android SDK was not found: set ANDROID_HOME (or ANDROID_SDK_ROOT) to it; see docs/android.md".to_string())
}

/// The SDK tools the build uses.
struct Tools {
    linker: PathBuf,
    strip: PathBuf,
    aapt2: PathBuf,
    aapt: PathBuf,
    zipalign: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
}

impl Tools {
    fn find(sdk: &Path) -> Result<Tools, String> {
        let ndk = match std::env::var_os("ANDROID_NDK_HOME").filter(|p| !p.is_empty()).map(PathBuf::from) {
            Some(p) if p.is_dir() => p,
            _ => newest(&sdk.join("ndk")).ok_or_else(|| {
                format!("no NDK: set ANDROID_NDK_HOME or install one (`sdkmanager --sdk_root={} \"ndk;27.2.12479018\"`)", sdk.display())
            })?,
        };
        let prebuilt = ndk.join("toolchains/llvm/prebuilt").join(ndk_host());
        let linker = prebuilt.join("bin").join(format!("aarch64-linux-android{MIN_SDK}-clang{}", if cfg!(windows) { ".cmd" } else { "" }));
        if !linker.is_file() {
            return Err(format!("the NDK linker {} is missing (NDK r21 or newer with the {} prebuilt)", linker.display(), ndk_host()));
        }
        let strip = prebuilt.join("bin").join(exe("llvm-strip"));
        if !strip.is_file() {
            return Err(format!("{} is missing from the NDK", strip.display()));
        }
        let build_tools = newest(&sdk.join("build-tools"))
            .ok_or_else(|| format!("no build-tools: `sdkmanager --sdk_root={} \"build-tools;{TARGET_SDK}.0.0\"`", sdk.display()))?;
        let platform = newest(&sdk.join("platforms"))
            .ok_or_else(|| format!("no platform: `sdkmanager --sdk_root={} \"platforms;android-{TARGET_SDK}\"`", sdk.display()))?;
        let android_jar = platform.join("android.jar");
        let tool = |name: &str, script: bool| {
            let p = build_tools.join(if script && cfg!(windows) { format!("{name}.bat") } else { exe(name) });
            if p.is_file() { Ok(p) } else { Err(format!("{} is missing from the build-tools", p.display())) }
        };
        Ok(Tools {
            linker,
            strip,
            aapt2: tool("aapt2", false)?,
            aapt: tool("aapt", false)?,
            zipalign: tool("zipalign", false)?,
            apksigner: tool("apksigner", true)?,
            android_jar,
        })
    }
}

/// The NDK's prebuilt folder for this machine (Apple silicon uses the x86_64 one under Rosetta).
fn ndk_host() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(windows) {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// The highest-versioned folder in `dir` (`34.0.0`, `android-34`, `27.2.12479018`: compared number
/// by number).
fn newest(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(Vec<u64>, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (version_numbers(&e.file_name().to_string_lossy()), e.path()))
        .collect();
    found.sort();
    found.pop().map(|(_, p)| p)
}

/// The numbers in a folder name, in order (`android-34` → `[34]`, `27.2.12479018` → `[27, 2, 12479018]`).
pub fn version_numbers(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Android's integer version from `major.minor.patch` (`0.13.0` → 1300; room for 99 minors and
/// patches).
pub fn version_code(version: &str) -> u64 {
    let n = version_numbers(version);
    n.first().copied().unwrap_or(0) * 10_000 + n.get(1).copied().unwrap_or(0) * 100 + n.get(2).copied().unwrap_or(0)
}

fn workspace_version_and_target() -> Result<(String, PathBuf), String> {
    let m = metadata()?;
    let version = m["packages"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["name"] == "lightcraft-android"))
        .and_then(|p| p["version"].as_str())
        .ok_or("cargo metadata: lightcraft-android not found")?
        .to_string();
    let target = m["target_directory"].as_str().ok_or("cargo metadata: no target_directory")?;
    Ok((version, PathBuf::from(target)))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_codes_order_releases() {
        assert_eq!(version_code("0.13.0"), 1300);
        assert_eq!(version_code("0.13.2"), 1302);
        assert_eq!(version_code("1.0.0"), 10_000);
        assert!(version_code("0.14.0") > version_code("0.13.99"));
    }

    #[test]
    fn folder_versions_compare_number_by_number() {
        assert_eq!(version_numbers("android-34"), [34]);
        assert_eq!(version_numbers("27.2.12479018"), [27, 2, 12479018]);
        assert!(version_numbers("34.0.0") > version_numbers("9.0.0"), "not a string compare");
    }
}
