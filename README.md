# MOGE (Mobi Own Game Engine)

| Crate | Purpose |
|---|---|
| `moge-core` | Values, API-Dump-driven classes, instance tree, editor (undo/redo, live edits), project format, script-file loader, file browser |
| `moge-formats` | rbxlx / rbxl / rbxm importers and rbxlx writer |
| `moge-math` | Matrices and orbit camera |
| `moge-scene` | Render scene builder, picking, Roblox sun/sky parameters, viewport tool math |
| `moge-gui` | UI layout engine (UDim2, anchors, ZIndex, clipping), drag/resize/snap math |
| `moge-luau` | Luau runtime on mlua (separate workspace; builds Luau from C++). Optional `luau` feature of `moge-app`, on by default |
| `moge-script` | Luau tokenizer, colorizer, syntax diagnostics, API-Dump-driven autocomplete |
| `moge-roblox` | Roblox web API requests/parsers (OAuth PKCE, thumbnails, asset download, publish) |
| `moge-app` | Android/desktop app: wgpu renderer (procedural Roblox-like sky) + egui UI. Separate workspace |
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
Drag to orbit, scroll to zoom, right/middle drag to pan. On Android: one finger orbits (or drags a gizmo handle), two fingers pinch-zoom and pan.

## Using the app
* **Script files**: File > Import scripts (.lua .luau .txt), or Script tab > Load file (replaces the open script's source). `.server/.client/.module` suffixes and a trailing `return` pick the class.
* **Files on Android 11+**: grant *All files access* to MOGE in system settings, or use the *App folder* shortcut in the file browser.
* **API dump**: embedded when CI can download it, otherwise downloaded on first start and cached.
* **Login/publish**: register an OAuth app in Creator Hub (scopes `openid profile universe-places:write`), put its client id and redirect URL in Settings. After signing in, paste the address of the final page into the app. An Open Cloud API key also works for publishing.

## Running Luau (▶ Run / ■ Stop)
Real Luau via mlua. Run executes every enabled Script/LocalScript on a **snapshot** of the project (your project is never modified) and prints to the Output panel. Supported: `print/warn/error/pcall`, `task.spawn/defer/delay/wait/cancel`, `wait/spawn/delay`, `require` (ModuleScript), `Instance.new`, instance properties via the API Dump (type-checked), `GetChildren/FindFirstChild/Clone/Destroy/...`, attributes, `ChildAdded/ChildRemoved`, BindableEvent `Event/Fire`, `Vector3/Vector2/Color3/UDim/UDim2/CFrame/Enum`. Errors are reported with script name and line.
The first CI build with mlua is untested: if the APK step fails, CI falls back to a build without Luau and prints a warning.

## Not included yet
* Physics, rendering and player objects in scripts (Players, Humanoid, RunService, TweenService, DataStores, RemoteEvents, HTTP).
* `Signal:Wait()`, `WaitForChild` waiting, `GetPropertyChangedSignal`, `Changed`/`Touched` and other engine events (accepted by `:Connect`, never fired; a warning is logged once).
* Custom sky textures (the sky is procedural), part textures/decals, MeshPart/Union geometry (drawn as blocks), terrain.
* zstd-compressed rbxl chunks, PhysicalProperties/attributes round trip.
* Android system file picker (SAF): the app has its own browser.

