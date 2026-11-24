//! MOGE app shell: window, touch/mouse input and the render loop. Android entry point is `android_main`.
mod gfx;

use gfx::Gfx;
use moge_math::OrbitCamera;
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct App {
    gfx: Option<Gfx>,
    cam: OrbitCamera,
    touches: HashMap<u64, (f64, f64)>,
    mouse_down: bool,
    mouse_last: Option<(f64, f64)>,
}

impl App {
    fn touch(&mut self, t: Touch) {
        let (x, y) = (t.location.x, t.location.y);
        match t.phase {
            TouchPhase::Started => {
                self.touches.insert(t.id, (x, y));
            }
            TouchPhase::Moved => {
                let old = self.touches.insert(t.id, (x, y));
                let Some((ox, oy)) = old else { return };
                match self.touches.len() {
                    1 => self.cam.orbit((x - ox) as f32, (y - oy) as f32),
                    2 => {
                        let other = self.touches.iter().find(|(k, _)| **k != t.id).map(|(_, v)| *v);
                        if let Some((px, py)) = other {
                            let before = (ox - px).hypot(oy - py);
                            let after = (x - px).hypot(y - py);
                            if before > 1.0 && after > 1.0 {
                                self.cam.zoom((before / after) as f32);
                            }
                        }
                    }
                    _ => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.touches.remove(&t.id);
            }
        }
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
        self.touches.clear();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = &mut self.gfx {
                    g.resize(s.width, s.height);
                }
            }
            WindowEvent::Touch(t) => self.touch(t),
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.mouse_down = state == ElementState::Pressed;
                if !self.mouse_down {
                    self.mouse_last = None;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x, position.y);
                if self.mouse_down {
                    if let Some((lx, ly)) = self.mouse_last {
                        self.cam.orbit((p.0 - lx) as f32, (p.1 - ly) as f32);
                    }
                    self.mouse_last = Some(p);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                self.cam.zoom(0.9f32.powf(y));
            }
            WindowEvent::RedrawRequested => {
                if let Some(g) = &mut self.gfx {
                    g.render(&self.cam);
                    g.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

pub fn run(event_loop: EventLoop<()>) {
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { cam: OrbitCamera::default(), ..Default::default() };
    if let Err(e) = event_loop.run_app(&mut app) {
        log::error!("Event loop terminated with an error: {e}");
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    match EventLoop::builder().with_android_app(app).build() {
        Ok(el) => run(el),
        Err(e) => log::error!("Failed to create the event loop: {e}"),
    }
}
