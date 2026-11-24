# MOGE (Mobi Own Game Engine)

| Crate | Purpose |
|---|---|
| `moge-core` | Values, API-Dump-driven classes, instance tree, asset ids (rbxassetid / rbxthumb) |
| `moge-formats` | rbxlx, rbxl and rbxm importers (`examples/inspect.rs` inspects real files) |
| `moge-math` | Matrices and orbit camera |
| `moge-app` | Android/desktop app shell (winit + wgpu). Separate workspace |
| `moge-desktop` | Desktop runner for `moge-app` (kept apart so cargo-apk does not package it) |

## Tests (no network needed)
`cargo test --workspace`

## APK via GitHub Actions (no PC required)
1. Create a repository and push this folder as the repository root.
2. Open the **Actions** tab, run **Android APK** (it also runs on every push to `main`).
3. When it finishes, download the `MOGE-apk` artifact, unzip it and install `MOGE.apk`.

If the build fails, copy the failing step's log; the first compile of `moge-app` is the least verified part.

## APK locally
Requires Android SDK, NDK 26, Java 17 and the Rust target:
```
rustup target add aarch64-linux-android
cargo install cargo-apk --locked
cd crates/moge-app && cargo apk build        # APK in target/debug/apk/
cargo apk run                                # installs and launches on a connected device
```

## Desktop preview
`cd crates/moge-desktop && cargo run --release`
Drag to orbit, scroll to zoom. On Android: one finger orbits, two fingers zoom.
