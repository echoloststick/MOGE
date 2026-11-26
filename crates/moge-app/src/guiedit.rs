//! UI editor: renders a ScreenGui with the layout engine, selection, drag/resize with guides, snapping and
//! device presets. Layout math lives in `moge-gui`; this file only draws and translates gestures.
use crate::studio::*;
use egui::{Align2, Color32, FontId, PointerButton, Pos2, Rect, Sense, Stroke, Ui};
use moge_core::{Id, Tree, Value};
use moge_gui::{GuiNode, Mode};

const HANDLES: [(i8, i8); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
const PALETTE: &[&str] = &["Frame", "TextLabel", "TextButton", "TextBox", "ImageLabel", "ImageButton", "ScrollingFrame", "UICorner", "UIStroke", "UIListLayout", "UIGridLayout", "UIPadding", "UIAspectRatioConstraint", "UIGradient"];

fn c32(c: [f32; 3], a: f32) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(a))
}

fn grect(r: &moge_gui::Rect, origin: Pos2, zoom: f32) -> Rect {
    Rect::from_min_size(origin + egui::vec2(r.x, r.y) * zoom, egui::vec2(r.w, r.h) * zoom)
}

impl Studio {
    fn gv_f(&self, id: Id, name: &str, d: f32) -> f32 {
        match self.ed.get_value(id, name) {
            Some(Value::Float(f)) => f as f32,
            Some(Value::Int(i)) => i as f32,
            _ => d,
        }
    }
    fn gv_c(&self, id: Id, name: &str, d: [f32; 3]) -> [f32; 3] {
        match self.ed.get_value(id, name) {
            Some(Value::Color3(c)) => c,
            _ => d,
        }
    }
    fn gv_s(&self, id: Id, name: &str) -> String {
        match self.ed.get_value(id, name) {
            Some(Value::String(s)) => s,
            _ => String::new(),
        }
    }
    fn gv_e(&self, id: Id, name: &str) -> u32 {
        match self.ed.get_value(id, name) {
            Some(Value::Enum(e)) => e,
            _ => 0,
        }
    }
    fn gv_b(&self, id: Id, name: &str, d: bool) -> bool {
        match self.ed.get_value(id, name) {
            Some(Value::Bool(b)) => b,
            _ => d,
        }
    }
    fn gv_u2(&self, id: Id, name: &str) -> (f32, f32, f32, f32) {
        match self.ed.get_value(id, name) {
            Some(Value::UDim2 { sx, ox, sy, oy }) => (sx, ox as f32, sy, oy as f32),
            _ => (0.0, 0.0, 0.0, 0.0),
        }
    }

    fn gui_roots(&self) -> Vec<Id> {
        let t = &self.ed.tree;
        let Some(sg) = t.find_first_child_of_class(Tree::ROOT, "StarterGui") else { return Vec::new() };
        t.node(sg).map(|n| n.children.iter().copied().filter(|c| t.node(*c).map(|n| n.class == "ScreenGui").unwrap_or(false)).collect()).unwrap_or_default()
    }

    fn child_of_class(&self, id: Id, class: &str) -> Option<Id> {
        self.ed.tree.find_first_child_of_class(id, class)
    }

    pub fn gui_ui(&mut self, ui: &mut Ui, _rect: Rect) {
        let roots = self.gui_roots();
        if self.gui.root.map(|r| !self.ed.tree.is_alive(r)).unwrap_or(true) {
            self.gui.root = roots.first().copied();
        }
        ui.horizontal_wrapped(|ui| {
            let cur = self.gui.root.and_then(|r| self.ed.tree.name(r)).unwrap_or("(no ScreenGui)").to_string();
            egui::ComboBox::from_id_salt("gui_root").selected_text(cur).show_ui(ui, |ui| {
                for r in &roots {
                    let n = self.ed.tree.name(*r).unwrap_or("ScreenGui").to_string();
                    if ui.selectable_label(self.gui.root == Some(*r), n).clicked() {
                        self.gui.root = Some(*r);
                    }
                }
            });
            if ui.button("New ScreenGui").clicked() {
                self.insert.parent = None;
                self.insert_class("ScreenGui");
                self.gui.root = self.ed.selection.first().copied();
            }
            egui::ComboBox::from_id_salt("gui_dev").selected_text(DEVICES[self.gui.device].0).show_ui(ui, |ui| {
                for (i, d) in DEVICES.iter().enumerate() {
                    if ui.selectable_label(self.gui.device == i, d.0).clicked() {
                        self.gui.device = i;
                        self.gui.fit_pending = true;
                    }
                }
            });
            ui.selectable_value(&mut self.gui.mode, Mode::Offset, "Offset");
            ui.selectable_value(&mut self.gui.mode, Mode::Scale, "Scale");
            ui.toggle_value(&mut self.gui.snap, "Snap");
            if ui.button("Fit").clicked() {
                self.gui.fit_pending = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            for c in PALETTE {
                if ui.small_button(*c).clicked() {
                    self.insert.parent = None;
                    self.insert_class(c);
                }
            }
        });
        let canvas = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(canvas, Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 0.0, Color32::from_gray(28));
        let Some(root) = self.gui.root else {
            painter.text(canvas.center(), Align2::CENTER_CENTER, "Create a ScreenGui (button above) to start designing.", FontId::proportional(16.0), Color32::from_gray(160));
            return;
        };
        let (sw, sh) = (DEVICES[self.gui.device].1, DEVICES[self.gui.device].2);
        if self.gui.fit_pending && canvas.width() > 10.0 {
            self.gui.zoom = ((canvas.width() - 40.0) / sw).min((canvas.height() - 40.0) / sh).clamp(0.05, 4.0);
            self.gui.pan = egui::Vec2::ZERO;
            self.gui.fit_pending = false;
        }
        let zoom = self.gui.zoom;
        let origin = canvas.center() - egui::vec2(sw, sh) * zoom / 2.0 + self.gui.pan;
        let screen = Rect::from_min_size(origin, egui::vec2(sw, sh) * zoom);
        painter.rect_filled(screen, 0.0, Color32::from_gray(64));
        painter.rect_stroke(screen, 0.0, Stroke::new(1.0, Color32::from_gray(110)));
        let nodes = moge_gui::layout(&self.ed.tree, root, (sw, sh));

        // ---- gestures
        if let Some(mt) = ui.ctx().input(|i| i.multi_touch()) {
            if mt.zoom_delta.is_finite() && mt.zoom_delta > 0.0 {
                self.gui.zoom = (self.gui.zoom * mt.zoom_delta).clamp(0.05, 6.0);
            }
            self.gui.pan += mt.translation_delta;
            self.gui.drag = None;
            self.ed.live_end();
        } else {
            if resp.hovered() {
                let s = ui.ctx().input(|i| i.raw_scroll_delta.y);
                if s != 0.0 {
                    self.gui.zoom = (self.gui.zoom * 1.002f32.powf(s)).clamp(0.05, 6.0);
                }
            }
            if resp.drag_started_by(PointerButton::Primary) {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let d = self.gui_begin_drag(&nodes, pos, origin, zoom, (sw, sh));
                    if let GuiDrag::Move { id, .. } = &d {
                        if !self.ed.selection.contains(id) {
                            self.ed.select(vec![*id]);
                        }
                    }
                    self.gui.drag = Some(d);
                }
            }
            if resp.dragged_by(PointerButton::Primary) {
                let d = resp.drag_delta() / zoom;
                self.gui_apply_drag(&nodes, d, zoom, resp.drag_delta());
            }
            if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
                self.gui.pan += resp.drag_delta();
            }
            if resp.drag_stopped() {
                self.gui.drag = None;
                self.gui.guides.clear();
                self.ed.live_end();
            }
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let p = (pos - origin) / zoom;
                    match moge_gui::hit_test(&nodes, p.x, p.y) {
                        Some(id) => {
                            if self.multi {
                                self.ed.toggle_select(id);
                            } else {
                                self.ed.select(vec![id]);
                            }
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

        // ---- draw
        let zoom = self.gui.zoom;
        let origin = canvas.center() - egui::vec2(sw, sh) * zoom / 2.0 + self.gui.pan;
        for n in &nodes {
            self.draw_gui_node(&painter, canvas, n, origin, zoom);
        }
        let blue = Color32::from_rgb(70, 160, 255);
        for n in nodes.iter().filter(|n| self.ed.selection.contains(&n.id)) {
            painter.rect_stroke(grect(&n.rect, origin, zoom), 0.0, Stroke::new(2.0, blue));
        }
        if let Some(first) = self.ed.selection.first().and_then(|s| nodes.iter().find(|n| n.id == *s)) {
            let r = grect(&first.rect, origin, zoom);
            for (hx, hy) in HANDLES {
                let c = handle_pos(r, hx, hy);
                let hr = Rect::from_center_size(c, egui::vec2(14.0, 14.0));
                painter.rect_filled(hr, 2.0, Color32::WHITE);
                painter.rect_stroke(hr, 2.0, Stroke::new(1.5, blue));
            }
        }
        for g in &self.gui.guides {
            let s = Stroke::new(1.0, Color32::from_rgb(255, 80, 200));
            match g {
                moge_gui::Guide::Vertical(x) => {
                    painter.line_segment([Pos2::new(origin.x + x * zoom, screen.top()), Pos2::new(origin.x + x * zoom, screen.bottom())], s);
                }
                moge_gui::Guide::Horizontal(y) => {
                    painter.line_segment([Pos2::new(screen.left(), origin.y + y * zoom), Pos2::new(screen.right(), origin.y + y * zoom)], s);
                }
            }
        }
    }

    fn gui_begin_drag(&self, nodes: &[GuiNode], pos: Pos2, origin: Pos2, zoom: f32, screen: (f32, f32)) -> GuiDrag {
        let find_parent_rect = |id: Id| -> moge_gui::Rect {
            let parent = self.ed.tree.node(id).and_then(|n| n.parent);
            parent.and_then(|p| nodes.iter().find(|n| n.id == p)).map(|n| n.rect).unwrap_or(moge_gui::Rect::new(0.0, 0.0, screen.0, screen.1))
        };
        if let Some(first) = self.ed.selection.first().and_then(|s| nodes.iter().find(|n| n.id == *s)) {
            let r = grect(&first.rect, origin, zoom);
            for (hx, hy) in HANDLES {
                if handle_pos(r, hx, hy).distance(pos) <= 22.0 {
                    let a = match self.ed.get_value(first.id, "AnchorPoint") {
                        Some(Value::Vector2(v)) => v,
                        _ => [0.0, 0.0],
                    };
                    return GuiDrag::Resize {
                        id: first.id,
                        handle: (hx, hy),
                        pos0: self.gv_u2(first.id, "Position"),
                        size0: self.gv_u2(first.id, "Size"),
                        anchor: a,
                        parent: find_parent_rect(first.id),
                        current: (first.rect.w, first.rect.h),
                        total: (0.0, 0.0),
                    };
                }
            }
        }
        let p = (pos - origin) / zoom;
        if let Some(id) = moge_gui::hit_test(nodes, p.x, p.y) {
            if let Some(n) = nodes.iter().find(|n| n.id == id) {
                return GuiDrag::Move { id, pos0: self.gv_u2(id, "Position"), parent: find_parent_rect(id), rect0: n.rect, total: (0.0, 0.0) };
            }
        }
        GuiDrag::Pan
    }

    fn gui_apply_drag(&mut self, nodes: &[GuiNode], d: egui::Vec2, _zoom: f32, raw: egui::Vec2) {
        let mode = self.gui.mode;
        let snap = self.gui.snap;
        let zoom = self.gui.zoom;
        let mut edit: Option<Vec<(Id, String, Value)>> = None;
        let mut guides = Vec::new();
        match self.gui.drag.as_mut() {
            Some(GuiDrag::Pan) => self.gui.pan += raw,
            Some(GuiDrag::Move { id, pos0, parent, rect0, total }) => {
                total.0 += d.x;
                total.1 += d.y;
                let (mut dx, mut dy) = *total;
                if snap {
                    let others: Vec<moge_gui::Rect> = nodes.iter().filter(|n| n.id != *id && self.ed.tree.node(n.id).and_then(|x| x.parent) == self.ed.tree.node(*id).and_then(|x| x.parent)).map(|n| n.rect).collect();
                    let moving = moge_gui::Rect::new(rect0.x + dx, rect0.y + dy, rect0.w, rect0.h);
                    let ((cx, cy), g) = moge_gui::snap_to_guides(moving, &others, *parent, 6.0 / zoom);
                    dx += cx;
                    dy += cy;
                    guides = g;
                }
                let (sx, ox, sy, oy) = moge_gui::move_udim2(*pos0, dx, dy, (parent.w, parent.h), mode);
                let (ox, oy) = if mode == Mode::Offset { (ox.round(), oy.round()) } else { (ox, oy) };
                edit = Some(vec![(*id, "Position".into(), Value::UDim2 { sx, ox: ox as i32, sy, oy: oy as i32 })]);
            }
            Some(GuiDrag::Resize { id, handle, pos0, size0, anchor, parent, current, total }) => {
                total.0 += d.x;
                total.1 += d.y;
                let (p, s) = moge_gui::resize_udim2(*pos0, *size0, *anchor, *handle, total.0, total.1, (parent.w, parent.h), mode, *current, 4.0);
                let r = |v: f32| if mode == Mode::Offset { v.round() } else { v };
                edit = Some(vec![
                    (*id, "Position".into(), Value::UDim2 { sx: p.0, ox: r(p.1) as i32, sy: p.2, oy: r(p.3) as i32 }),
                    (*id, "Size".into(), Value::UDim2 { sx: s.0, ox: r(s.1) as i32, sy: s.2, oy: r(s.3) as i32 }),
                ]);
            }
            None => {}
        }
        self.gui.guides = guides;
        if let Some(items) = edit {
            if let Err(e) = self.ed.live_edit(|ed| ed.set_many("Edit UI", items)) {
                self.log(Level::Warn, format!("UI edit failed: {e}"));
            }
        }
    }

    fn draw_gui_node(&mut self, painter: &egui::Painter, canvas: Rect, n: &GuiNode, origin: Pos2, zoom: f32) {
        let id = n.id;
        let Some(class) = self.ed.tree.node(id).map(|x| x.class.clone()) else { return };
        let rect = grect(&n.rect, origin, zoom);
        let clip = grect(&n.clip, origin, zoom).intersect(canvas);
        if clip.width() <= 0.0 || clip.height() <= 0.0 {
            return;
        }
        let p = painter.with_clip_rect(clip);
        let corner = self.child_of_class(id, "UICorner").map(|c| match self.ed.get_value(c, "CornerRadius") {
            Some(Value::UDim(s, o)) => (s * n.rect.w.min(n.rect.h) + o as f32).max(0.0) * zoom,
            _ => 8.0 * zoom,
        });
        let r = corner.unwrap_or(0.0).min(rect.width().min(rect.height()) / 2.0);
        let bg_a = 1.0 - self.gv_f(id, "BackgroundTransparency", 0.0);
        if bg_a > 0.001 {
            p.rect_filled(rect, r, c32(self.gv_c(id, "BackgroundColor3", [0.64, 0.64, 0.64]), bg_a));
        }
        if matches!(class.as_str(), "ImageLabel" | "ImageButton") {
            let key = self.gv_s(id, "Image");
            let tint = c32(self.gv_c(id, "ImageColor3", [1.0, 1.0, 1.0]), 1.0 - self.gv_f(id, "ImageTransparency", 0.0));
            if key.is_empty() {
                p.rect_stroke(rect.shrink(2.0), 0.0, Stroke::new(1.0, Color32::from_gray(150)));
                p.line_segment([rect.left_top(), rect.right_bottom()], Stroke::new(1.0, Color32::from_gray(120)));
                p.line_segment([rect.right_top(), rect.left_bottom()], Stroke::new(1.0, Color32::from_gray(120)));
            } else {
                self.request_image(&key);
                match self.images.get(&key) {
                    Some(Img::Ready(tex)) => {
                        p.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    }
                    Some(Img::Loading) => {
                        p.text(rect.center(), Align2::CENTER_CENTER, "loading image...", FontId::proportional(12.0), Color32::from_gray(200));
                    }
                    _ => {
                        p.text(rect.center(), Align2::CENTER_CENTER, "image unavailable", FontId::proportional(12.0), Color32::from_rgb(230, 120, 120));
                    }
                }
            }
        }
        // border: UIStroke wins over BorderSizePixel
        if let Some(sid) = self.child_of_class(id, "UIStroke") {
            let th = self.gv_f(sid, "Thickness", 1.0) * zoom;
            if th > 0.0 {
                p.rect_stroke(rect, r, Stroke::new(th.max(1.0), c32(self.gv_c(sid, "Color", [0.0, 0.0, 0.0]), 1.0 - self.gv_f(sid, "Transparency", 0.0))));
            }
        } else {
            let bs = self.gv_f(id, "BorderSizePixel", 1.0);
            if bs > 0.0 && bg_a > 0.001 {
                p.rect_stroke(rect, r, Stroke::new((bs * zoom).max(1.0), c32(self.gv_c(id, "BorderColor3", [0.1, 0.1, 0.1]), 1.0)));
            }
        }
        if matches!(class.as_str(), "TextLabel" | "TextButton" | "TextBox") {
            let mut text = self.gv_s(id, "Text");
            let mut color = c32(self.gv_c(id, "TextColor3", [0.1, 0.1, 0.1]), 1.0 - self.gv_f(id, "TextTransparency", 0.0));
            if text.is_empty() && class == "TextBox" {
                text = self.gv_s(id, "PlaceholderText");
                color = c32(self.gv_c(id, "PlaceholderColor3", [0.7, 0.7, 0.7]), 1.0);
            }
            if !text.is_empty() {
                let size = if self.gv_b(id, "TextScaled", false) { (n.rect.h * 0.7).max(4.0) } else { self.gv_f(id, "TextSize", 14.0) } * zoom;
                let (ax, ay) = (self.gv_e(id, "TextXAlignment"), self.gv_e(id, "TextYAlignment"));
                // Roblox: X Left=0 Right=1 Center=2; Y Top=0 Center=1 Bottom=2
                let h_align = match ax {
                    0 => egui::Align::Min,
                    1 => egui::Align::Max,
                    _ => egui::Align::Center,
                };
                let v_align = match ay {
                    0 => egui::Align::Min,
                    2 => egui::Align::Max,
                    _ => egui::Align::Center,
                };
                let pad = 3.0 * zoom;
                let inner = rect.shrink(pad);
                let wrap = if self.gv_b(id, "TextWrapped", false) { inner.width().max(1.0) } else { f32::INFINITY };
                let galley = p.layout(text, FontId::proportional(size.max(1.0)), color, wrap);
                let gs = galley.size();
                let x = match h_align {
                    egui::Align::Min => inner.left(),
                    egui::Align::Max => inner.right() - gs.x,
                    _ => inner.center().x - gs.x / 2.0,
                };
                let y = match v_align {
                    egui::Align::Min => inner.top(),
                    egui::Align::Max => inner.bottom() - gs.y,
                    _ => inner.center().y - gs.y / 2.0,
                };
                p.galley(Pos2::new(x, y), galley, color);
            }
        }
    }
}

fn handle_pos(r: Rect, hx: i8, hy: i8) -> Pos2 {
    Pos2::new(
        match hx {
            -1 => r.left(),
            0 => r.center().x,
            _ => r.right(),
        },
        match hy {
            -1 => r.top(),
            0 => r.center().y,
            _ => r.bottom(),
        },
    )
}
