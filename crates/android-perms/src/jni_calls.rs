//! The JNI calls. `PackageManager.PERMISSION_GRANTED` is 0.

use android_activity::AndroidApp;
use jni::objects::{JObject, JValue};
use jni::{JNIEnv, JavaVM};

const PERMISSION_GRANTED: i32 = 0;

/// Whether every one of `perms` is granted (`Activity.checkSelfPermission`).
pub fn granted(app: &AndroidApp, perms: &[&str]) -> Result<bool, String> {
    with_activity(app, |env, activity| {
        for p in perms {
            let name: JObject = env.new_string(p)?.into();
            let r = env.call_method(activity, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&name)])?.i()?;
            if r != PERMISSION_GRANTED {
                return Ok(false);
            }
        }
        Ok(true)
    })
}

/// Show the system's permission dialog for `perms` (`Activity.requestPermissions`). Returns when
/// the dialog is up; the answer is not forwarded to native code, so ask [`granted`] again later.
/// A permission the user refused with "don't ask again" shows no dialog: the app's settings page
/// is then the only way, and the caller says so.
pub fn request(app: &AndroidApp, perms: &[&str]) -> Result<(), String> {
    with_activity(app, |env, activity| {
        let len = i32::try_from(perms.len()).map_err(|_| jni::errors::Error::WrongJValueType("len", "i32"))?;
        let array = env.new_object_array(len, "java/lang/String", JObject::null())?;
        for (i, p) in perms.iter().enumerate() {
            let name = env.new_string(p)?;
            env.set_object_array_element(&array, i as i32, name)?;
        }
        let array: JObject = array.into();
        env.call_method(activity, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&array), JValue::Int(1)])?;
        Ok(())
    })
}

/// Run `f` with a JNI environment attached to this thread and the activity object. A Java
/// exception raised inside is cleared (it would abort the process at the next JNI call) and
/// returned as the error.
fn with_activity<T>(app: &AndroidApp, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Result<T, String> {
    // SAFETY: `vm_as_ptr` is the process's one JavaVM, valid for the life of the process;
    // `from_raw` only wraps the pointer and takes no ownership of the VM.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| format!("JavaVM: {e}"))?;
    let mut env = vm.attach_current_thread().map_err(|e| format!("attaching the thread to the JVM: {e}"))?;
    // SAFETY: `activity_as_ptr` is a JNI global reference to the NativeActivity that stays valid
    // while `app` does (android-activity's documented contract); a plain `JObject` neither owns
    // nor deletes it (only `GlobalRef` / `AutoLocal` would), and it is used only inside `f`.
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let result = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return Err(match result {
            Err(e) => format!("Android refused: {e}"),
            Ok(_) => "Android refused (a Java exception)".to_string(),
        });
    }
    result.map_err(|e| format!("JNI: {e}"))
}
