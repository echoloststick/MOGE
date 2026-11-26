//! MOGE app shell: window, egui frame loop, GPU renderer. Android entry point is `android_main`.
mod gfx;
mod guiedit;
mod input;
mod mesh;
mod net;
mod panels;
mod script;
mod studio;
mod viewport;

use gfx::{EguiInput, Gfx, SceneInput};
use input::Input;
use std::path::PathBuf;
use std::time::Instant;
use studio::{Studio, Tab};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Where the app keeps its files and which folders the file browser offers first.
pub struct Dirs {
    pub data: PathBuf,
    pub start: Vec<(String, PathBuf)>,
}

impl Dirs {
    pub fn desktop() -> Dirs {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let mut start = vec![("Home".to_string(), home.clone())];
        for (n, d) in [("Documents", "Documents"), ("Downloads", "Downloads"), ("Desktop", "Desktop")] {
            let p = home.join(d);
            if p.is_dir() {
                start.push((n.to_string(), p));
            }
        }
        Dirs { data: home.join(".moge"), start }
    }
}

fn new_context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut(|s| {
        s.spacing.interact_size = egui::vec2(40.0, 34.0);
        s.spacing.button_padding = egui::vec2(10.0, 6.0);
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.scroll.bar_width = 10.0;
        for (st, size) in [(egui::TextStyle::Body, 16.0), (egui::TextStyle::Button, 16.0), (egui::TextStyle::Small, 12.0), (egui::TextStyle::Heading, 22.0)] {
            s.text_styles.insert(st, egui::FontId::proportional(size));
        }
        s.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(15.0));
    });
    ctx
}

struct App {
    gfx: Option<Gfx>,
    ctx: egui::Context,
    input: Input,
    studio: Studio,
    start: Instant,
    uploaded_rev: u64,
    #[cfg(target_os = "android")]
    android: Option<winit::platform::android::activity::AndroidApp>,
    #[cfg(target_os = "android")]
    system_kb: bool,
}

impl App {
    fn new(dirs: Dirs) -> App {
        App {
            gfx: None,
            ctx: new_context(),
            input: Input::default(),
            studio: Studio::new(dirs.data, dirs.start),
            start: Instant::now(),
            uploaded_rev: u64::MAX,
            #[cfg(target_os = "android")]
            android: None,
            #[cfg(target_os = "android")]
            system_kb: false,
        }
    }

    fn frame(&mut self) {
        let Some(g) = self.gfx.as_mut() else { return };
        let ppp = g.window.scale_factor() as f32;
        self.ctx.set_pixels_per_point(ppp);
        let (w, h) = g.size();
        let mut raw = self.input.take((w, h), ppp, self.start.elapsed().as_secs_f64());
        raw.events.extend(self.studio.inject.drain(..));
        let studio = &mut self.studio;
        let out = self.ctx.run(raw, |ctx| {
            studio.poll_net(ctx);
            studio.autosave();
            studio.ui(ctx);
            if !ctx.wants_keyboard_input() && !ctx.input(|i| i.pointer.any_down()) {
                studio.ed.live_end();
            }
        });
        if !out.platform_output.copied_text.is_empty() {
            studio.clipboard = out.platform_output.copied_text.clone();
        }
        if let Some(u) = &out.platform_output.open_url {
            if let Err(e) = webbrowser::open(&u.url) {
                log::warn!("Could not open {}: {e}", u.url);
            }
        }
        #[cfg(target_os = "android")]
        {
            let wants = self.ctx.wants_keyboard_input() && !studio.settings.vkb;
            if wants != self.system_kb {
                self.system_kb = wants;
                if let Some(a) = &self.android {
                    if wants {
                        a.show_soft_input(true);
                    } else {
                        a.hide_soft_input(true);
                    }
                }
            }
        }
        let view3d = studio.tab == Tab::View3d;
        if view3d && (studio.refresh_scene() || self.uploaded_rev != studio.scene_rev) {
            g.upload_scene(&studio.scene);
            self.uploaded_rev = studio.scene_rev;
        }
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        g.render(&SceneInput { cam: &studio.cam, lighting: &studio.scene.lighting, viewport: studio.viewport_px, draw3d: view3d }, EguiInput { prims, textures: out.textures_delta, ppp });
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let window = match el.create_window(Window::default_attributes().with_title("MOGE")) {
            Ok(w) => std::sync::Arc::new(w),
            Err(e) => {
                log::error!("Failed to create the window: {e}");
                el.exit();
                return;
            }
        };
        match pollster::block_on(Gfx::new(window)) {
            Ok(g) => {
                // A new renderer knows none of the old egui textures, so the UI context and image cache restart too.
                self.ctx = new_context();
                self.studio.images.clear();
                self.uploaded_rev = u64::MAX;
                g.window.request_redraw();
                self.gfx = Some(g);
            }
            Err(e) => {
                log::error!("GPU initialization failed: {e}");
                el.exit();
            }
        }
    }

    /// Android destroys the native window when the app goes to the background; drop the surface with it.
    fn suspended(&mut self, _el: &ActiveEventLoop) {
        self.gfx = None;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(g) = &self.gfx {
            self.input.on_event(&event, g.window.scale_factor() as f32);
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = &mut self.gfx {
                    g.resize(s.width, s.height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

pub fn run_with(event_loop: EventLoop<()>, dirs: Dirs) {
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(dirs);
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Event loop terminated with an error: {e}");
    }
}

pub fn run(event_loop: EventLoop<()>) {
    run_with(event_loop, Dirs::desktop());
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    let internal = app.internal_data_path().unwrap_or_else(|| PathBuf::from("/data/local/tmp"));
    let external = app.external_data_path();
    let mut start: Vec<(String, PathBuf)> = Vec::new();
    if let Some(e) = &external {
        start.push(("App folder".into(), e.clone()));
    }
    start.push(("App private".into(), internal.clone()));
    for (n, p) in [("Storage", "/storage/emulated/0"), ("Download", "/storage/emulated/0/Download"), ("Documents", "/storage/emulated/0/Documents")] {
        start.push((n.into(), PathBuf::from(p)));
    }
    let dirs = Dirs { data: internal, start };
    match EventLoop::builder().with_android_app(app.clone()).build() {
        Ok(el) => {
            let mut a = App::new(dirs);
            a.android = Some(app);
            el.set_control_flow(ControlFlow::Poll);
            if let Err(e) = el.run_app(&mut a) {
                log::error!("Event loop terminated with an error: {e}");
            }
        }
        Err(e) => log::error!("Failed to create the event loop: {e}"),
    }
}
