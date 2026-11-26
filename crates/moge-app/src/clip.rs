//! Android clipboard access (egui has no paste gesture on touch screens).
#[cfg(target_os = "android")]
pub fn clipboard_text(app: &winit::platform::android::activity::AndroidApp) -> Option<String> {
    use jni::objects::{JObject, JString, JValue};
    use jni::JavaVM;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let act = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let name = env.new_string("clipboard").ok()?;
    let svc = env.call_method(&act, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)]).ok()?.l().ok()?;
    let clip = env.call_method(&svc, "getPrimaryClip", "()Landroid/content/ClipData;", &[]).ok()?.l().ok()?;
    if clip.as_raw().is_null() {
        return None;
    }
    let item = env.call_method(&clip, "getItemAt", "(I)Landroid/content/ClipData$Item;", &[JValue::Int(0)]).ok()?.l().ok()?;
    let cs = env.call_method(&item, "coerceToText", "(Landroid/content/Context;)Ljava/lang/CharSequence;", &[JValue::Object(&act)]).ok()?.l().ok()?;
    if cs.as_raw().is_null() {
        return None;
    }
    let s = env.call_method(&cs, "toString", "()Ljava/lang/String;", &[]).ok()?.l().ok()?;
    let js = JString::from(s);
    let text: String = env.get_string(&js).ok()?.into();
    Some(text)
}

#[cfg(target_os = "android")]
pub fn set_clipboard_text(app: &winit::platform::android::activity::AndroidApp, text: &str) -> Option<()> {
    use jni::objects::{JObject, JValue};
    use jni::JavaVM;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let act = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    let name = env.new_string("clipboard").ok()?;
    let svc = env.call_method(&act, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)]).ok()?.l().ok()?;
    let label = env.new_string("MOGE").ok()?;
    let txt = env.new_string(text).ok()?;
    let clip = env
        .call_static_method("android/content/ClipData", "newPlainText", "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;", &[JValue::Object(&label), JValue::Object(&txt)])
        .ok()?
        .l()
        .ok()?;
    env.call_method(&svc, "setPrimaryClip", "(Landroid/content/ClipData;)V", &[JValue::Object(&clip)]).ok()?;
    Some(())
}
