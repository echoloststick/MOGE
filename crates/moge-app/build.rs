//! Embeds assets/Full-API-Dump.json when present (CI downloads it); otherwise the app fetches it at runtime.
fn main() {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let src = std::path::Path::new("assets/Full-API-Dump.json");
    let dst = std::path::Path::new(&out).join("embedded_dump.json");
    if src.exists() {
        std::fs::copy(src, &dst).expect("failed to copy the embedded API dump");
    } else {
        std::fs::write(&dst, "").expect("failed to write the placeholder API dump");
    }
    println!("cargo:rerun-if-changed=assets/Full-API-Dump.json");
    println!("cargo:rerun-if-changed=build.rs");
}
