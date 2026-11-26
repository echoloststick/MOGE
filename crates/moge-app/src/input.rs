//! winit -> egui input translation (touch, mouse, keyboard, IME).
use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{Key as WKey, NamedKey};

#[derive(Default)]
pub struct Input {
    pub events: Vec<Event>,
    modifiers: Modifiers,
    pointer_touch: Option<u64>,
}

fn named(k: NamedKey) -> Option<Key> {
    Some(match k {
        NamedKey::Backspace => Key::Backspace,
        NamedKey::Delete => Key::Delete,
        NamedKey::Enter => Key::Enter,
        NamedKey::Tab => Key::Tab,
        NamedKey::Escape => Key::Escape,
        NamedKey::Space => Key::Space,
        NamedKey::ArrowLeft => Key::ArrowLeft,
        NamedKey::ArrowRight => Key::ArrowRight,
        NamedKey::ArrowUp => Key::ArrowUp,
        NamedKey::ArrowDown => Key::ArrowDown,
        NamedKey::Home => Key::Home,
        NamedKey::End => Key::End,
        NamedKey::PageUp => Key::PageUp,
        NamedKey::PageDown => Key::PageDown,
        _ => return None,
    })
}

impl Input {
    pub fn on_event(&mut self, ev: &WindowEvent, ppp: f32) {
        let pt = |x: f64, y: f64| Pos2::new(x as f32 / ppp, y as f32 / ppp);
        let m = self.modifiers;
        match ev {
            WindowEvent::ModifiersChanged(mods) => {
                let s = mods.state();
                self.modifiers = Modifiers { alt: s.alt_key(), ctrl: s.control_key(), shift: s.shift_key(), mac_cmd: false, command: s.control_key() };
            }
            WindowEvent::CursorMoved { position, .. } => self.events.push(Event::PointerMoved(pt(position.x, position.y))),
            WindowEvent::CursorLeft { .. } => self.events.push(Event::PointerGone),
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => PointerButton::Primary,
                    MouseButton::Right => PointerButton::Secondary,
                    MouseButton::Middle => PointerButton::Middle,
                    _ => return,
                };
                // The position comes from the preceding CursorMoved event.
                self.events.push(Event::PointerButton { pos: self.last_pos(), button: b, pressed: *state == ElementState::Pressed, modifiers: m });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, d) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(*x, *y)),
                    MouseScrollDelta::PixelDelta(p) => (egui::MouseWheelUnit::Point, egui::vec2(p.x as f32 / ppp, p.y as f32 / ppp)),
                };
                self.events.push(Event::MouseWheel { unit, delta: d, modifiers: m });
            }
            WindowEvent::Touch(t) => {
                let pos = pt(t.location.x, t.location.y);
                let phase = match t.phase {
                    TouchPhase::Started => egui::TouchPhase::Start,
                    TouchPhase::Moved => egui::TouchPhase::Move,
                    TouchPhase::Ended => egui::TouchPhase::End,
                    TouchPhase::Cancelled => egui::TouchPhase::Cancel,
                };
                self.events.push(Event::Touch { device_id: egui::TouchDeviceId(0), id: egui::TouchId::from(t.id), phase, pos, force: None });
                match t.phase {
                    TouchPhase::Started => {
                        if self.pointer_touch.is_none() {
                            self.pointer_touch = Some(t.id);
                            self.events.push(Event::PointerMoved(pos));
                            self.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: m });
                        }
                    }
                    TouchPhase::Moved => {
                        if self.pointer_touch == Some(t.id) {
                            self.events.push(Event::PointerMoved(pos));
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        if self.pointer_touch == Some(t.id) {
                            self.pointer_touch = None;
                            self.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: m });
                            self.events.push(Event::PointerGone);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key = match &event.logical_key {
                    WKey::Named(n) => named(*n),
                    WKey::Character(c) => Key::from_name(&c.to_uppercase()),
                    _ => None,
                };
                if let Some(key) = key {
                    self.events.push(Event::Key { key, physical_key: None, pressed, repeat: event.repeat, modifiers: m });
                }
                if pressed && !m.ctrl && !m.command {
                    if let Some(t) = &event.text {
                        let t: String = t.chars().filter(|c| !c.is_control()).collect();
                        if !t.is_empty() {
                            self.events.push(Event::Text(t));
                        }
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if !text.is_empty() {
                    self.events.push(Event::Text(text.clone()));
                }
            }
            _ => {}
        }
    }

    fn last_pos(&self) -> Pos2 {
        self.events
            .iter()
            .rev()
            .find_map(|e| if let Event::PointerMoved(p) = e { Some(*p) } else { None })
            .unwrap_or(Pos2::ZERO)
    }

    pub fn take(&mut self, size_px: (u32, u32), ppp: f32, time: f64) -> RawInput {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(size_px.0 as f32 / ppp, size_px.1 as f32 / ppp));
        RawInput { screen_rect: Some(rect), time: Some(time), modifiers: self.modifiers, events: std::mem::take(&mut self.events), focused: true, ..Default::default() }
    }
}
