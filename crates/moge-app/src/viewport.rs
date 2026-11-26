//! 3D viewport interaction: camera gestures, picking, transform handles and selection overlay.
//! The scene itself is drawn by the GPU behind this transparent panel.
use crate::studio::*;
use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Ui};
use moge_core::value::IDENTITY_ROT;
use moge_core::{Id, Value};
use moge_math::Mat4;
use moge_scene::tools::{self, Tool};

const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(235, 70, 70), Color32::from_rgb(80, 200, 90), Color32::from_rgb(80, 130, 245)];
const WORLD_AXES: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

impl Studio {
    /// Gizmo origin, axes and world length for the current selection.
    fn gizmo(&self) -> Option<([f32; 3], [[f32; 3]; 3], f32)> {
        if self.tool == Tool::Select {
            return None;
        }
        let parts = self.scene.parts_under(&self.ed.tree, &self.ed.selection);
        let first = parts.first()?;
        let (origin, _) = self.scene.bounds(&self.ed.tree, &self.ed.selection)?;
        let axes = if self.tool == Tool::Scale {
            let r = first.rot;
            [[r[0], r[3], r[6]], [r[1], r[4], r[7]], [r[2], r[5], r[8]]]
        } else {
            WORLD_AXES
        };
        let e = self.cam.eye();
        let d = ((e[0] - origin[0]).powi(2) + (e[1] - origin[1]).powi(2) + (e[2] - origin[2]).powi(2)).sqrt();
        Some((origin, axes, (d * 0.2).max(0.5)))
    }

    pub fn viewport_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let ppp = ui.ctx().pixels_per_point();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let (w, h) = (rect.width(), rect.height());
        if w < 2.0 || h < 2.0 {
            return;
        }
        let vp = self.cam.view_proj(w / h);

        // ---- gestures
        if let Some(mt) = ui.ctx().input(|i| i.multi_touch()) {
            if mt.zoom_delta.is_finite() && mt.zoom_delta > 0.0 {
                self.cam.zoom(1.0 / mt.zoom_delta);
            }
            self.cam.pan(mt.translation_delta.x * ppp, mt.translation_delta.y * ppp);
            self.drag3d = None;
            self.ed.live_end();
        } else {
            if resp.hovered() {
                let s = ui.ctx().input(|i| i.raw_scroll_delta.y);
                if s != 0.0 {
                    self.cam.zoom(0.995f32.powf(s));
                }
            }
            if resp.drag_started_by(PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let l = (pos - rect.min).to_pos2();
                    self.drag3d = Some(self.begin_drag(&vp, l, w, h));
                }
            }
            if resp.dragged_by(PointerButton::Primary) {
                let d = resp.drag_delta();
                match &mut self.drag3d {
                    Some(Drag3d::Orbit) => self.cam.orbit(d.x * ppp, d.y * ppp),
                    Some(Drag3d::Handle { total, .. }) => {
                        total.0 += d.x;
                        total.1 += d.y;
                        self.apply_handle(&vp, w, h);
                    }
                    None => {}
                }
            }
            if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
                let d = resp.drag_delta();
                self.cam.pan(d.x * ppp, d.y * ppp);
            }
            if resp.drag_stopped() {
                self.drag3d = None;
                self.ed.live_end();
            }
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let l = (pos - rect.min).to_pos2();
                    let (o, d) = self.cam.ray(l.x, l.y, w, h);
                    match self.scene.pick(o, d) {
                        Some((id, _)) => {
                            if self.multi {
                                self.ed.toggle_select(id);
                            } else {
                                self.ed.select(vec![id]);
                            }
                            self.reveal(id);
                        }
                        None => {
                            if !self.multi {
                                self.ed.select(Vec::new());
                            }
                        }
                    }
                }
            }
        }

        // ---- overlay
        let painter = ui.painter_at(rect);
        let proj = |p: [f32; 3]| tools::project(&vp, p, w, h).map(|(x, y)| Pos2::new(rect.min.x + x, rect.min.y + y));
        let outline = Stroke::new(1.5, Color32::from_rgb(70, 160, 255));
        let mut drawn = 0;
        for p in self.scene.parts_under(&self.ed.tree, &self.ed.selection) {
            drawn += 1;
            if drawn > 400 {
                break;
            }
            let c = moge_scene::corners(p);
            for i in 0..8usize {
                for bit in [1usize, 2, 4] {
                    if i & bit == 0 {
                        if let (Some(a), Some(b)) = (proj(c[i]), proj(c[i | bit])) {
                            painter.line_segment([a, b], outline);
                        }
                    }
                }
            }
        }
        if let Some((origin, axes, len)) = self.gizmo() {
            if let Some(o) = proj(origin) {
                let active = if let Some(Drag3d::Handle { axis, .. }) = &self.drag3d { Some(*axis) } else { None };
                for (i, ax) in axes.iter().enumerate() {
                    let end = [origin[0] + ax[0] * len, origin[1] + ax[1] * len, origin[2] + ax[2] * len];
                    let Some(e) = proj(end) else { continue };
                    let hot = active == Some(i);
                    let col = if hot { Color32::WHITE } else { AXIS_COLORS[i] };
                    painter.line_segment([o, e], Stroke::new(if hot { 5.0 } else { 3.0 }, col));
                    match self.tool {
                        Tool::Scale => {
                            painter.rect_filled(Rect::from_center_size(e, egui::vec2(18.0, 18.0)), 2.0, col);
                        }
                        Tool::Rotate => {
                            painter.circle_stroke(e, 11.0, Stroke::new(3.0, col));
                            let n = 48;
                            let (u, v) = perpendicular(*ax);
                            let r = len * 0.8;
                            let pts: Vec<Pos2> = (0..=n)
                                .filter_map(|k| {
                                    let a = std::f32::consts::TAU * k as f32 / n as f32;
                                    let q = [origin[0] + (u[0] * a.cos() + v[0] * a.sin()) * r, origin[1] + (u[1] * a.cos() + v[1] * a.sin()) * r, origin[2] + (u[2] * a.cos() + v[2] * a.sin()) * r];
                                    proj(q)
                                })
                                .collect();
                            painter.add(egui::Shape::line(pts, Stroke::new(1.5, col.gamma_multiply(0.7))));
                        }
                        _ => {
                            painter.circle_filled(e, 10.0, col);
                        }
                    }
                }
            }
        }
        // camera controls (useful without multi-touch)
        egui::Area::new(egui::Id::new("cam_buttons")).fixed_pos(rect.right_top() + egui::vec2(-56.0, 10.0)).show(ui.ctx(), |ui| {
            if ui.add_sized([44.0, 44.0], egui::Button::new("+")).clicked() {
                self.cam.zoom(0.8);
            }
            if ui.add_sized([44.0, 44.0], egui::Button::new("-")).clicked() {
                self.cam.zoom(1.25);
            }
            if ui.add_sized([44.0, 44.0], egui::Button::new("F")).clicked() {
                self.focus_selection();
            }
        });
    }

    fn begin_drag(&self, vp: &Mat4, local: Pos2, w: f32, h: f32) -> Drag3d {
        if let Some((origin, axes, len)) = self.gizmo() {
            if let Some(axis) = tools::pick_handle(vp, origin, &axes, len, local.x, local.y, w, h, 26.0) {
                let parts = self.scene.parts_under(&self.ed.tree, &self.ed.selection).iter().map(|p| (p.id, p.pos, p.rot, p.size)).collect();
                return Drag3d::Handle { axis, total: (0.0, 0.0), origin, axes, parts };
            }
        }
        Drag3d::Orbit
    }

    fn apply_handle(&mut self, vp: &Mat4, w: f32, h: f32) {
        let Some(Drag3d::Handle { axis, total, origin, axes, parts }) = &self.drag3d else { return };
        let (ai, total, origin, axes, parts) = (*axis, *total, *origin, *axes, parts.clone());
        let ax = axes[ai];
        let st = &self.settings;
        let mut items: Vec<(Id, String, Value)> = Vec::new();
        match self.tool {
            Tool::Move => {
                let amt = tools::axis_amount(vp, origin, ax, total.0, total.1, w, h);
                for (id, pos, rot, _) in &parts {
                    items.push((*id, "CFrame".into(), Value::CFrame { pos: tools::move_by(*pos, ax, amt, st.snap_move), rot: *rot }));
                }
            }
            Tool::Scale => {
                let amt = tools::axis_amount(vp, origin, ax, total.0, total.1, w, h);
                for (id, pos, rot, size) in &parts {
                    let local = [rot[ai], rot[3 + ai], rot[6 + ai]];
                    let ns = tools::scale_by(*size, ai, amt, st.snap_scale);
                    let d = (ns[ai] - size[ai]) / 2.0;
                    let np = [pos[0] + local[0] * d, pos[1] + local[1] * d, pos[2] + local[2] * d];
                    items.push((*id, "Size".into(), Value::Vector3(ns)));
                    items.push((*id, "CFrame".into(), Value::CFrame { pos: np, rot: *rot }));
                }
            }
            Tool::Rotate => {
                let deg = total.0 * 0.5;
                let r = tools::rotate_by(&IDENTITY_ROT, ax, deg, st.snap_rot);
                for (id, pos, rot, _) in &parts {
                    let rel = [pos[0] - origin[0], pos[1] - origin[1], pos[2] - origin[2]];
                    let rp = [r[0] * rel[0] + r[1] * rel[1] + r[2] * rel[2], r[3] * rel[0] + r[4] * rel[1] + r[5] * rel[2], r[6] * rel[0] + r[7] * rel[1] + r[8] * rel[2]];
                    let np = [origin[0] + rp[0], origin[1] + rp[1], origin[2] + rp[2]];
                    items.push((*id, "CFrame".into(), Value::CFrame { pos: np, rot: tools::rotate_by(rot, ax, deg, st.snap_rot) }));
                }
            }
            Tool::Select => return,
        }
        if let Err(e) = self.ed.live_edit(|e| e.set_many("Transform", items)) {
            self.log(Level::Warn, format!("Transform failed: {e}"));
        }
    }
}

fn perpendicular(a: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let helper = if a[1].abs() < 0.9 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let cross = |p: [f32; 3], q: [f32; 3]| [p[1] * q[2] - p[2] * q[1], p[2] * q[0] - p[0] * q[2], p[0] * q[1] - p[1] * q[0]];
    let norm = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let u = norm(cross(a, helper));
    let v = norm(cross(a, u));
    (u, v)
}
