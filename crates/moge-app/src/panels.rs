//! Top-level layout and the docked panels: menus, Explorer, Properties, Output, Insert hub, Toolbox, dialogs.
use crate::studio::*;
use egui::{Align2, Color32, Context, FontId, Sense, Ui};
use moge_core::editor::PropRow;
use moge_core::{Id, Tree, Value};
use moge_scene::tools::Tool;

const ROW_H: f32 = 32.0;
const COMMON: &[&str] = &["Part", "WedgePart", "Model", "Folder", "Script", "LocalScript", "ModuleScript", "ScreenGui", "Frame", "TextLabel", "TextButton", "ImageLabel", "ScrollingFrame", "UICorner", "UIListLayout", "PointLight", "Sound", "RemoteEvent"];

pub fn class_color(dump: &moge_core::ApiDump, class: &str) -> Color32 {
    if dump.is_a(class, "BasePart") || class == "Part" {
        Color32::from_rgb(90, 150, 230)
    } else if class == "Model" || class == "Folder" {
        Color32::from_rgb(110, 190, 120)
    } else if dump.is_a(class, "LuaSourceContainer") || class.ends_with("Script") {
        Color32::from_rgb(230, 200, 90)
    } else if dump.is_a(class, "GuiObject") || dump.is_a(class, "LayerCollector") || class.starts_with("UI") {
        Color32::from_rgb(180, 120, 230)
    } else if dump.class(class).map(|c| c.service).unwrap_or(false) {
        Color32::from_rgb(150, 150, 160)
    } else {
        Color32::from_rgb(200, 140, 90)
    }
}

pub fn value_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{f}"),
        Value::String(s) => s.clone(),
        Value::Vector2(a) => format!("{}, {}", a[0], a[1]),
        Value::Vector3(a) | Value::Color3(a) => format!("{}, {}, {}", a[0], a[1], a[2]),
        Value::UDim(s, o) => format!("{s}, {o}"),
        Value::UDim2 { sx, ox, sy, oy } => format!("{{{sx}, {ox}}}, {{{sy}, {oy}}}"),
        Value::CFrame { pos, .. } => format!("CFrame({}, {}, {})", pos[0], pos[1], pos[2]),
        Value::Enum(n) => n.to_string(),
        Value::BrickColor(n) => format!("BrickColor {n}"),
        Value::Ref(Some(i)) => format!("Instance #{i}"),
        Value::Ref(None) => "nil".into(),
        Value::Font { family, .. } => family.rsplit('/').next().unwrap_or(family).to_string(),
        other => format!("<{}>", other.type_name()),
    }
}

/// Large tappable key used by the on-screen keyboard; never steals text focus.
fn key_button(ui: &mut Ui, label: &str, size: egui::Vec2) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense { click: true, drag: false, focusable: false });
    let vis = ui.style().interact(&resp);
    ui.painter().rect(rect.shrink(2.0), 6.0, vis.bg_fill, vis.bg_stroke);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(18.0), vis.text_color());
    resp.clicked()
}

impl Studio {
    pub fn ui(&mut self, ctx: &Context) {
        self.track_keyboard(ctx);
        if !self.settings.vkb && ctx.wants_keyboard_input() {
            egui::Area::new(egui::Id::new("paste_btn")).anchor(Align2::RIGHT_TOP, [-8.0, 52.0]).order(egui::Order::Foreground).show(ctx, |ui| {
                if key_button(ui, "Paste", egui::vec2(64.0, 34.0)) {
                    self.paste_req = true;
                }
            });
        }
        self.top_bar(ctx);
        if self.settings.vkb && self.vkb_open && ctx.wants_keyboard_input() {
            self.vkb(ctx);
        }
        if self.show_left && self.tab != Tab::Script {
            egui::SidePanel::left("left").resizable(true).default_width(270.0).width_range(180.0..=520.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.side, Side::Explorer, "Explorer");
                    ui.selectable_value(&mut self.side, Side::Toolbox, "Toolbox");
                });
                ui.separator();
                match self.side {
                    Side::Explorer => self.explorer(ui),
                    Side::Toolbox => self.toolbox(ui),
                }
            });
        }
        if self.show_right && self.tab != Tab::Script {
            egui::SidePanel::right("right").resizable(true).default_width(300.0).width_range(200.0..=560.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.right, Right::Properties, "Properties");
                    ui.selectable_value(&mut self.right, Right::Output, "Output");
                });
                ui.separator();
                match self.right {
                    Right::Properties => self.properties(ui),
                    Right::Output => self.output_panel(ui),
                }
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            let ppp = ctx.pixels_per_point();
            self.viewport_px = [rect.min.x * ppp, rect.min.y * ppp, rect.width() * ppp, rect.height() * ppp];
            match self.tab {
                Tab::View3d => self.viewport_ui(ui, rect),
                Tab::Gui => self.gui_ui(ui, rect),
                Tab::Script => self.script_ui(ui, rect),
            }
        });
        self.insert_window(ctx);
        self.file_window(ctx);
        self.account_window(ctx);
        self.settings_window(ctx);
        self.confirm_window(ctx);
        if !self.dump_ready {
            egui::Area::new(egui::Id::new("dump_banner")).anchor(Align2::CENTER_TOP, [0.0, 56.0]).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label("The Roblox API dump is not loaded yet (downloading). Properties and search need it.");
                    if ui.button("Retry download").clicked() {
                        self.download_dump();
                    }
                });
            });
        }
    }

    fn track_keyboard(&mut self, ctx: &Context) {
        let wants = ctx.wants_keyboard_input();
        let focused = ctx.memory(|m| m.focused());
        match (wants, focused) {
            (true, Some(f)) => {
                if self.kb_last != Some(f) {
                    self.vkb_open = true;
                    self.kb_last = Some(f);
                }
                self.kb_focus = Some(f);
            }
            (_, None) => self.kb_last = None,
            _ => {}
        }
    }

    // ---------------------------------------------------------------- top bar
    fn top_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.run_cancel.is_some() {
                    if ui.button("■ Stop").clicked() {
                        self.stop_run();
                    }
                } else if ui.button("▶ Run").clicked() {
                    self.start_run();
                }
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        if self.ed.dirty {
                            self.confirm_new = true;
                        } else {
                            self.new_project();
                        }
                        ui.close_menu();
                    }
                    if ui.button("Open...").clicked() {
                        self.open_dialog(FileAction::OpenProject);
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        match self.project_path.clone() {
                            Some(p) => self.save_project(&p),
                            None => self.open_dialog(FileAction::SaveProject),
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save as...").clicked() {
                        self.open_dialog(FileAction::SaveProject);
                        ui.close_menu();
                    }
                    if ui.button("Export .rbxlx...").clicked() {
                        self.open_dialog(FileAction::ExportRbxlx);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Import scripts (.lua .luau .txt)...").clicked() {
                        self.open_dialog(FileAction::ImportScripts);
                        ui.close_menu();
                    }
                    if ui.button("Import model / place file...").clicked() {
                        self.open_dialog(FileAction::ImportModel);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Restore autosave").clicked() {
                        self.restore_autosave();
                        ui.close_menu();
                    }
                    if ui.button("Update API dump").clicked() {
                        self.download_dump();
                        ui.close_menu();
                    }
                    if ui.button("Account / Publish...").clicked() {
                        self.show_account = true;
                        ui.close_menu();
                    }
                    if ui.button("Settings...").clicked() {
                        self.show_settings = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(self.ed.can_undo(), egui::Button::new("Undo")).clicked() {
                        self.ed.undo();
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.ed.can_redo(), egui::Button::new("Redo")).clicked() {
                        self.ed.redo();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Focus selection").clicked() {
                        self.focus_selection();
                        ui.close_menu();
                    }
                    if ui.button("Select all in Workspace").clicked() {
                        if let Some(ws) = self.ed.tree.find_first_child_of_class(Tree::ROOT, "Workspace") {
                            let kids = self.ed.tree.node(ws).map(|n| n.children.clone()).unwrap_or_default();
                            self.ed.select(kids);
                        }
                        ui.close_menu();
                    }
                });
                if ui.button("+ Insert").clicked() {
                    self.insert.open = true;
                    self.insert.parent = None;
                }
                ui.separator();
                if self.tab == Tab::View3d {
                    for (t, n) in [(Tool::Select, "Select"), (Tool::Move, "Move"), (Tool::Scale, "Scale"), (Tool::Rotate, "Rotate")] {
                        ui.selectable_value(&mut self.tool, t, n);
                    }
                    ui.separator();
                }
                if ui.add_enabled(self.ed.can_undo(), egui::Button::new("Undo")).clicked() {
                    self.ed.undo();
                }
                if ui.add_enabled(self.ed.can_redo(), egui::Button::new("Redo")).clicked() {
                    self.ed.redo();
                }
                ui.toggle_value(&mut self.multi, "Multi");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::View3d, "3D");
                ui.selectable_value(&mut self.tab, Tab::Gui, "UI");
                ui.selectable_value(&mut self.tab, Tab::Script, "Script");
                ui.separator();
                ui.toggle_value(&mut self.show_left, "Left");
                ui.toggle_value(&mut self.show_right, "Right");
                ui.label(format!("{}{}", self.project_name, if self.ed.dirty { " *" } else { "" }));
            });
        });
    }

    fn confirm_window(&mut self, ctx: &Context) {
        if !self.confirm_new {
            return;
        }
        egui::Window::new("Discard changes?").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("The project has unsaved changes. Create a new project anyway?");
            ui.horizontal(|ui| {
                if ui.button("Discard and create").clicked() {
                    self.new_project();
                    self.confirm_new = false;
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_new = false;
                }
            });
        });
    }

    // ---------------------------------------------------------------- explorer
    fn flat_tree(&self) -> Vec<(Id, usize)> {
        let mut out = Vec::new();
        let mut stack: Vec<(Id, usize)> = self.ed.tree.node(Tree::ROOT).map(|n| n.children.iter().rev().map(|c| (*c, 0)).collect()).unwrap_or_default();
        while let Some((id, depth)) = stack.pop() {
            out.push((id, depth));
            if self.open_nodes.contains(&id) {
                if let Some(n) = self.ed.tree.node(id) {
                    stack.extend(n.children.iter().rev().map(|c| (*c, depth + 1)));
                }
            }
        }
        out
    }

    pub fn reveal(&mut self, id: Id) {
        let mut cur = self.ed.tree.node(id).and_then(|n| n.parent);
        while let Some(p) = cur {
            self.open_nodes.insert(p);
            cur = self.ed.tree.node(p).and_then(|n| n.parent);
        }
    }

    fn explorer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.explorer_filter).hint_text("Search objects").desired_width(ui.available_width() - 36.0));
            if ui.button("x").clicked() {
                self.explorer_filter.clear();
            }
        });
        self.action_bar(ui);
        if let Some(src) = &self.move_src {
            let n = src.len();
            let mut cancel = false;
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(240, 200, 80), format!("Tap the new parent for {n} object(s)"));
                cancel = ui.button("Cancel").clicked();
            });
            if cancel {
                self.move_src = None;
            }
        }
        ui.separator();
        let flat: Vec<(Id, usize)> = if self.explorer_filter.trim().is_empty() { self.flat_tree() } else { self.ed.find_by_name(&self.explorer_filter, 300).into_iter().map(|i| (i, 0)).collect() };
        let total = flat.len();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, total, |ui, range| {
            for i in range {
                if let Some((id, depth)) = flat.get(i).copied() {
                    self.explorer_row(ui, id, depth);
                }
            }
        });
    }

    fn action_bar(&mut self, ui: &mut Ui) {
        let sel = self.ed.selection.clone();
        let has = !sel.is_empty();
        ui.horizontal_wrapped(|ui| {
            let mut result: Result<(), String> = Ok(());
            if ui.add_enabled(has, egui::Button::new("Dup")).clicked() {
                result = self.ed.duplicate(&sel).map(|_| ());
            }
            if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
                result = self.ed.copy(&sel).map(|_| ());
            }
            if ui.add_enabled(has, egui::Button::new("Cut")).clicked() {
                result = self.ed.cut(&sel).map(|_| ());
            }
            if ui.add_enabled(self.ed.has_clipboard(), egui::Button::new("Paste")).clicked() {
                let parent = sel.first().copied().unwrap_or_else(|| self.ed.tree.find_first_child_of_class(Tree::ROOT, "Workspace").unwrap_or(Tree::ROOT));
                result = self.ed.paste(parent).map(|_| ());
            }
            if ui.add_enabled(has, egui::Button::new("Del")).clicked() {
                result = self.ed.delete(&sel).map(|_| ());
            }
            if ui.add_enabled(sel.len() > 1, egui::Button::new("Group")).clicked() {
                result = self.ed.group(&sel).map(|_| ());
            }
            if ui.add_enabled(has, egui::Button::new("Ungroup")).clicked() {
                result = self.ed.ungroup(&sel).map(|_| ());
            }
            if ui.add_enabled(has, egui::Button::new("Move to...")).clicked() {
                self.move_src = Some(sel.clone());
            }
            if ui.add_enabled(sel.len() == 1, egui::Button::new("Rename")).clicked() {
                let n = self.ed.tree.name(sel[0]).unwrap_or("").to_string();
                self.rename = Some((sel[0], n));
            }
            if ui.add_enabled(sel.len() == 1, egui::Button::new("+ Child")).clicked() {
                self.insert.open = true;
                self.insert.parent = Some(sel[0]);
            }
            if ui.add_enabled(has, egui::Button::new("Focus")).clicked() {
                self.focus_selection();
            }
            if sel.len() == 1 && self.is_script(sel[0]) && ui.button("Edit script").clicked() {
                self.open_script(sel[0]);
            }
            if let Err(e) = result {
                self.log(Level::Warn, e);
            }
        });
        if let Some((id, mut name)) = self.rename.take() {
            let mut keep = true;
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 120.0));
                if ui.button("OK").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    if let Err(e) = self.ed.rename(id, &name) {
                        self.log(Level::Warn, e);
                    }
                    keep = false;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.rename = Some((id, name));
            }
        }
    }

    fn explorer_row(&mut self, ui: &mut Ui, id: Id, depth: usize) {
        let Some(node) = self.ed.tree.node(id) else { return };
        let class = node.class.clone();
        let has_children = !node.children.is_empty();
        let name = self.ed.tree.name(id).unwrap_or("?").to_string();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), Sense::click());
        let selected = self.ed.selection.contains(&id);
        let p = ui.painter_at(rect);
        if selected {
            p.rect_filled(rect, 3.0, Color32::from_rgba_unmultiplied(70, 130, 230, 90));
        } else if resp.hovered() {
            p.rect_filled(rect, 3.0, Color32::from_white_alpha(10));
        }
        let indent = depth as f32 * 16.0;
        let ay = rect.center().y;
        if has_children {
            let x = rect.left() + indent + 10.0;
            let open = self.open_nodes.contains(&id);
            let pts = if open { vec![egui::pos2(x - 5.0, ay - 3.0), egui::pos2(x + 5.0, ay - 3.0), egui::pos2(x, ay + 4.0)] } else { vec![egui::pos2(x - 3.0, ay - 5.0), egui::pos2(x + 4.0, ay), egui::pos2(x - 3.0, ay + 5.0)] };
            p.add(egui::Shape::convex_polygon(pts, Color32::from_gray(190), egui::Stroke::NONE));
        }
        let ix = rect.left() + indent + 30.0;
        let icon = egui::Rect::from_center_size(egui::pos2(ix, ay), egui::vec2(16.0, 16.0));
        p.rect_filled(icon, 3.0, class_color(&self.ed.dump, &class));
        p.text(icon.center(), Align2::CENTER_CENTER, class.chars().next().unwrap_or('?'), FontId::proportional(11.0), Color32::BLACK);
        let tx = p.text(egui::pos2(ix + 14.0, ay), Align2::LEFT_CENTER, &name, FontId::proportional(15.0), Color32::from_gray(235));
        if class != name {
            p.text(egui::pos2(tx.right() + 8.0, ay), Align2::LEFT_CENTER, &class, FontId::proportional(11.0), Color32::from_gray(130));
        }
        if resp.clicked() {
            let on_arrow = has_children && resp.interact_pointer_pos().map(|q| q.x < rect.left() + indent + 22.0).unwrap_or(false);
            if on_arrow {
                if !self.open_nodes.remove(&id) {
                    self.open_nodes.insert(id);
                }
            } else if let Some(src) = self.move_src.take() {
                match self.ed.reparent(&src, id, None) {
                    Ok(()) => {
                        self.open_nodes.insert(id);
                    }
                    Err(e) => self.log(Level::Warn, format!("Cannot move there: {e}")),
                }
            } else if self.multi {
                self.ed.toggle_select(id);
            } else {
                self.ed.select(vec![id]);
            }
        }
        if resp.double_clicked() && self.is_script(id) {
            self.open_script(id);
        }
    }

    // ---------------------------------------------------------------- properties
    fn properties(&mut self, ui: &mut Ui) {
        let Some(id) = self.ed.selection.first().copied().filter(|i| self.ed.tree.is_alive(*i)) else {
            ui.label("Nothing selected. Tap an object in the Explorer or the 3D view.");
            return;
        };
        let ids = self.ed.selection.clone();
        let class = self.ed.tree.node(id).map(|n| n.class.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.strong(&class);
            if ids.len() > 1 {
                ui.label(format!("(+{} more; edits apply to all)", ids.len() - 1));
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.prop_filter).hint_text("Filter properties").desired_width(f32::INFINITY));
        let filter = self.prop_filter.trim().to_lowercase();
        let rows: Vec<PropRow> = self.ed.properties(id).into_iter().filter(|r| filter.is_empty() || r.name.to_lowercase().contains(&filter)).collect();
        if rows.is_empty() && !self.dump_ready {
            ui.label("Waiting for the API dump...");
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut groups: Vec<String> = Vec::new();
            for r in &rows {
                if !groups.contains(&r.group) {
                    groups.push(r.group.clone());
                }
            }
            for g in groups {
                let open_default = matches!(g.as_str(), "Appearance" | "Data" | "Transform") || !filter.is_empty();
                egui::CollapsingHeader::new(if g.is_empty() { "Other" } else { g.as_str() }).id_salt(("grp", &g)).default_open(open_default).show(ui, |ui| {
                    egui::Grid::new(("props", &g)).num_columns(2).striped(true).spacing([8.0, 6.0]).show(ui, |ui| {
                        for r in rows.iter().filter(|r| r.group == g) {
                            ui.label(&r.name);
                            self.prop_editor(ui, id, &ids, r);
                            ui.end_row();
                        }
                    });
                });
            }
        });
    }

    fn prop_editor(&mut self, ui: &mut Ui, id: Id, ids: &[Id], row: &PropRow) {
        if row.read_only {
            ui.label(value_text(&row.value));
            return;
        }
        let mut new: Option<Value> = None;
        let mut live = false;
        match &row.value {
            Value::Bool(b) => {
                let mut v = *b;
                if ui.checkbox(&mut v, "").changed() {
                    new = Some(Value::Bool(v));
                }
            }
            Value::Int(i) => {
                let mut v = *i;
                if ui.add(egui::DragValue::new(&mut v)).changed() {
                    new = Some(Value::Int(v));
                    live = true;
                }
            }
            Value::Float(f) => {
                let mut v = *f;
                if ui.add(egui::DragValue::new(&mut v).speed(0.05)).changed() {
                    new = Some(Value::Float(v));
                    live = true;
                }
            }
            Value::String(s) => {
                if s.len() > 300 || s.contains('\n') {
                    if self.is_script(id) && row.name == "Source" {
                        if ui.button("Open in editor").clicked() {
                            self.open_script(id);
                        }
                    } else {
                        ui.label(format!("({} chars)", s.len()));
                    }
                } else {
                    let key = egui::Id::new(("prop_text", id, &row.name));
                    let mut t: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| s.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(ui.available_width().max(120.0)));
                    if r.has_focus() {
                        ui.data_mut(|d| d.insert_temp(key, t.clone()));
                    }
                    if r.lost_focus() {
                        if t != *s {
                            new = Some(Value::String(t));
                        }
                        ui.data_mut(|d| d.remove_temp::<String>(key));
                    } else if !r.has_focus() {
                        ui.data_mut(|d| d.remove_temp::<String>(key));
                    }
                }
            }
            Value::Vector3(a) => {
                let mut v = *a;
                let mut ch = false;
                ui.horizontal(|ui| {
                    for k in 0..3 {
                        ch |= ui.add(egui::DragValue::new(&mut v[k]).speed(0.1)).changed();
                    }
                });
                if ch {
                    new = Some(Value::Vector3(v));
                    live = true;
                }
            }
            Value::Vector2(a) => {
                let mut v = *a;
                let mut ch = false;
                ui.horizontal(|ui| {
                    for k in 0..2 {
                        ch |= ui.add(egui::DragValue::new(&mut v[k]).speed(0.05)).changed();
                    }
                });
                if ch {
                    new = Some(Value::Vector2(v));
                    live = true;
                }
            }
            Value::Color3(a) => {
                let mut v = *a;
                if ui.color_edit_button_rgb(&mut v).changed() {
                    new = Some(Value::Color3(v));
                    live = true;
                }
            }
            Value::UDim(s, o) => {
                let (mut s2, mut o2) = (*s, *o);
                let mut ch = false;
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut s2).speed(0.01)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut o2)).changed();
                });
                if ch {
                    new = Some(Value::UDim(s2, o2));
                    live = true;
                }
            }
            Value::UDim2 { sx, ox, sy, oy } => {
                let (mut a, mut b, mut c, mut d) = (*sx, *ox, *sy, *oy);
                let mut ch = false;
                ui.horizontal_wrapped(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut a).speed(0.01).prefix("sx ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut b).prefix("ox ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut c).speed(0.01).prefix("sy ")).changed();
                    ch |= ui.add(egui::DragValue::new(&mut d).prefix("oy ")).changed();
                });
                if ch {
                    new = Some(Value::UDim2 { sx: a, ox: b, sy: c, oy: d });
                    live = true;
                }
            }
            Value::NumberRange(a, b) => {
                let (mut x, mut y) = (*a, *b);
                let mut ch = false;
                ui.horizontal(|ui| {
                    ch |= ui.add(egui::DragValue::new(&mut x).speed(0.1)).changed();
                    ch |= ui.add(egui::DragValue::new(&mut y).speed(0.1)).changed();
                });
                if ch {
                    new = Some(Value::NumberRange(x, y));
                    live = true;
                }
            }
            Value::BrickColor(n) => {
                let mut v = *n as i64;
                if ui.add(egui::DragValue::new(&mut v).range(1..=1032)).changed() {
                    new = Some(Value::BrickColor(v.max(1) as u32));
                    live = true;
                }
            }
            Value::Enum(cur) => {
                let items: Vec<(String, u32)> = self.ed.dump.enum_items(&row.type_name).to_vec();
                let cur_name = self.ed.dump.enum_item_name(&row.type_name, *cur).map(|s| s.to_string()).unwrap_or_else(|| cur.to_string());
                egui::ComboBox::from_id_salt(("enum", id, &row.name)).selected_text(cur_name).show_ui(ui, |ui| {
                    for (n, v) in &items {
                        if ui.selectable_label(*v == *cur, n).clicked() {
                            new = Some(Value::Enum(*v));
                        }
                    }
                });
            }
            other => {
                ui.label(value_text(other));
            }
        }
        if let Some(v) = new {
            let r = if live { self.ed.live_edit(|e| e.set_value(ids, &row.name, v)) } else { self.ed.set_value(ids, &row.name, v) };
            if let Err(e) = r {
                self.log(Level::Warn, format!("Cannot set {}: {e}", row.name));
            }
        }
    }

    fn output_panel(&mut self, ui: &mut Ui) {
        if ui.button("Clear").clicked() {
            self.output.clear();
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for (lvl, msg) in &self.output {
                let c = match lvl {
                    Level::Info => Color32::from_gray(210),
                    Level::Warn => Color32::from_rgb(240, 200, 80),
                    Level::Error => Color32::from_rgb(240, 100, 100),
                };
                ui.colored_label(c, msg);
            }
        });
    }

    // ---------------------------------------------------------------- insert hub
    fn smart_parent(&self, class: &str) -> Option<Id> {
        let t = &self.ed.tree;
        let sel = self.ed.selection.first().copied().filter(|i| t.is_alive(*i) && *i != Tree::ROOT);
        let is_gui_container = |i: Id| t.node(i).map(|n| moge_gui::is_gui_object(&n.class) || n.class == "ScreenGui").unwrap_or(false);
        if class == "ScreenGui" {
            return t.find_first_child_of_class(Tree::ROOT, "StarterGui").or(sel);
        }
        if moge_gui::is_gui_object(class) || class.starts_with("UI") {
            if let Some(s) = sel.filter(|s| is_gui_container(*s)) {
                return Some(s);
            }
            return t.descendants(Tree::ROOT).into_iter().find(|i| t.node(*i).map(|n| n.class == "ScreenGui").unwrap_or(false));
        }
        if class.ends_with("Script") && sel.is_none() {
            return Some(match class {
                "ModuleScript" => t.find_first_child_of_class(Tree::ROOT, "ReplicatedStorage"),
                "LocalScript" => t.find_first_child_of_class(Tree::ROOT, "StarterPlayer"),
                _ => t.find_first_child_of_class(Tree::ROOT, "ServerScriptService"),
            }
            .or_else(|| t.find_first_child_of_class(Tree::ROOT, "Workspace"))
            .unwrap_or(Tree::ROOT));
        }
        sel.or_else(|| t.find_first_child_of_class(Tree::ROOT, "Workspace"))
    }

    pub fn insert_class(&mut self, class: &str) {
        let parent = self.insert.parent.or_else(|| self.smart_parent(class));
        let Some(parent) = parent else {
            return self.log(Level::Warn, format!("No valid parent for {class}. Select a container first (for UI objects, create a ScreenGui)."));
        };
        match self.ed.insert(class, parent, self.cam.target) {
            Ok(id) => {
                self.ed.select(vec![id]);
                self.open_nodes.insert(parent);
                self.reveal(id);
                if self.is_script(id) {
                    self.open_script(id);
                }
            }
            Err(e) => self.log(Level::Warn, format!("Cannot insert {class}: {e}")),
        }
    }

    fn insert_window(&mut self, ctx: &Context) {
        if !self.insert.open {
            return;
        }
        let mut open = true;
        let mut chosen: Option<String> = None;
        egui::Window::new("Insert Object").open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).default_width(440.0).collapsible(false).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.insert.query).hint_text("Search classes (Part, Frame, Script...)").desired_width(f32::INFINITY));
            let target = self.insert.parent.or_else(|| self.ed.selection.first().copied()).and_then(|i| self.ed.tree.full_name(i)).unwrap_or_else(|| "automatic parent".into());
            ui.weak(format!("Parent: {target}"));
            let q = self.insert.query.trim().to_string();
            let results: Vec<String> = if q.is_empty() { COMMON.iter().map(|s| s.to_string()).collect() } else { self.ed.dump.search(&q, true, 80).iter().map(|c| c.name.clone()).collect() };
            egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for name in &results {
                        let color = class_color(&self.ed.dump, name);
                        if ui.add(egui::Button::new(egui::RichText::new(name).color(Color32::BLACK)).fill(color)).clicked() {
                            chosen = Some(name.clone());
                        }
                    }
                });
                if results.is_empty() {
                    ui.label(if self.dump_ready { "No matching creatable class." } else { "The API dump is not loaded yet." });
                }
            });
        });
        if let Some(c) = chosen {
            self.insert_class(&c);
            open = false;
        }
        self.insert.open = open;
    }

    // ---------------------------------------------------------------- toolbox
    fn toolbox(&mut self, ui: &mut Ui) {
        ui.label("Import by Place / Asset id");
        ui.add(egui::TextEdit::singleline(&mut self.toolbox.id_text).hint_text("e.g. 1818 or rbxassetid://1818").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.toolbox.as_model, true, "Insert as model");
            ui.selectable_value(&mut self.toolbox.as_model, false, "Merge as place");
        });
        if ui.button("Download and import").clicked() {
            self.import_asset_id();
        }
        if !self.toolbox.status.is_empty() {
            ui.label(&self.toolbox.status);
        }
        let recent = self.toolbox.recent.clone();
        if !recent.is_empty() {
            ui.separator();
            ui.weak("Recent");
            ui.horizontal_wrapped(|ui| {
                for r in recent {
                    if ui.small_button(&r).clicked() {
                        self.toolbox.id_text = r;
                    }
                }
            });
        }
        ui.separator();
        if ui.button("Open .rbxl / .rbxlx / .rbxm file...").clicked() {
            self.open_dialog(FileAction::ImportModel);
        }
        if ui.button("Import scripts (.lua .luau .txt)...").clicked() {
            self.open_dialog(FileAction::ImportScripts);
        }
        ui.separator();
        ui.weak("Places and models you do not own need the account cookie from Settings (stored locally, opt-in). Binary files compressed with zstd are not supported.");
    }

    // ---------------------------------------------------------------- file dialog
    fn file_window(&mut self, ctx: &Context) {
        let Some(action) = self.files.as_ref().map(|f| f.action) else { return };
        let mut open = true;
        let mut nav: Option<std::path::PathBuf> = None;
        let mut confirm: Option<std::path::PathBuf> = None;
        egui::Window::new(action.title()).open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).default_size([520.0, 440.0]).collapsible(false).show(ctx, |ui| {
            let starts = self.start_dirs.clone();
            ui.horizontal_wrapped(|ui| {
                for (n, p) in &starts {
                    if ui.small_button(n).clicked() {
                        nav = Some(p.clone());
                    }
                }
            });
            let Some(f) = self.files.as_mut() else { return };
            ui.horizontal(|ui| {
                if ui.button("Up").clicked() {
                    if let Some(p) = f.dir.parent() {
                        nav = Some(p.to_path_buf());
                    }
                }
                ui.label(f.dir.display().to_string());
            });
            if let Some(e) = &f.error {
                ui.colored_label(Color32::from_rgb(240, 120, 120), e);
            }
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, false]).show(ui, |ui| {
                for e in &f.entries {
                    let label = if e.is_dir { format!("[folder] {}", e.name) } else { format!("{}   ({} KB)", e.name, e.size / 1024) };
                    let picked = f.selected.contains(&e.path);
                    if ui.add_sized([ui.available_width(), 30.0], egui::SelectableLabel::new(picked, label)).clicked() {
                        if e.is_dir {
                            nav = Some(e.path.clone());
                        } else if action.is_save() {
                            f.name = e.name.clone();
                        } else if action.multi() {
                            if let Some(i) = f.selected.iter().position(|p| *p == e.path) {
                                f.selected.remove(i);
                            } else {
                                f.selected.push(e.path.clone());
                            }
                        } else {
                            confirm = Some(e.path.clone());
                        }
                    }
                }
                if f.entries.is_empty() && f.error.is_none() {
                    ui.weak("No matching files in this folder.");
                }
            });
            ui.separator();
            if action.is_save() {
                ui.add(egui::TextEdit::singleline(&mut f.name).hint_text("File name").desired_width(f32::INFINITY));
                if ui.button("Save here").clicked() && !f.name.trim().is_empty() {
                    confirm = Some(f.dir.join(f.name.trim()));
                }
            } else if action.multi() {
                let n = f.selected.len();
                if ui.add_enabled(n > 0, egui::Button::new(format!("Import {n} selected"))).clicked() {
                    confirm = f.selected.first().cloned();
                }
            }
        });
        if let Some(d) = nav {
            self.navigate(d);
        }
        if let Some(p) = confirm {
            self.dialog_confirm(p);
            return;
        }
        if !open {
            self.files = None;
        }
    }

    // ---------------------------------------------------------------- account and settings
    fn account_window(&mut self, ctx: &Context) {
        if !self.show_account {
            return;
        }
        let mut open = true;
        egui::Window::new("Account and Publish").open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).default_width(460.0).collapsible(false).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                match self.account.user.clone() {
                    Some(u) => {
                        ui.horizontal(|ui| {
                            ui.strong(format!("Signed in as {} ({})", u.name, u.id));
                            if ui.button("Sign out").clicked() {
                                self.logout();
                            }
                        });
                    }
                    None => {
                        ui.label("Sign in with Roblox (OAuth 2.0 + PKCE). Register an OAuth app in Creator Hub, put its client id in Settings and add the redirect URL shown there.");
                        if ui.button("Open Roblox sign-in").clicked() {
                            self.begin_login();
                        }
                        if !self.account.url.is_empty() {
                            ui.add(egui::TextEdit::singleline(&mut self.account.url.clone()).desired_width(f32::INFINITY));
                            ui.label("After signing in, paste the full address of the final page:");
                            ui.add(egui::TextEdit::singleline(&mut self.account.pasted).hint_text("http://localhost:3000/callback?code=...").desired_width(f32::INFINITY));
                            ui.horizontal(|ui| {
                                if ui.button("Paste").clicked() {
                                    self.inject.push(egui::Event::Paste(self.clipboard.clone()));
                                }
                                if ui.add_enabled(!self.account.busy, egui::Button::new("Finish sign-in")).clicked() {
                                    self.finish_login();
                                }
                            });
                        }
                    }
                }
                if !self.account.status.is_empty() {
                    ui.label(&self.account.status);
                }
                ui.separator();
                ui.strong("Publish");
                ui.checkbox(&mut self.account.use_api_key, "Use an Open Cloud API key instead of the signed-in account");
                let games = self.account.games.clone();
                if !games.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for g in games {
                            if ui.small_button(&g.name).clicked() {
                                self.settings.universe = g.universe_id.to_string();
                                self.settings.place = g.place_id.to_string();
                            }
                        }
                    });
                }
                egui::Grid::new("pubgrid").num_columns(2).show(ui, |ui| {
                    ui.label("Universe id");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.universe).desired_width(200.0));
                    ui.end_row();
                    ui.label("Place id");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.place).desired_width(200.0));
                    ui.end_row();
                });
                if ui.add_enabled(!self.account.busy, egui::Button::new("Publish current project")).clicked() {
                    self.save_settings();
                    self.publish();
                }
                ui.weak("Publishing overwrites the live place. Export a .rbxlx first if you need a backup.");
            });
        });
        self.show_account = open;
    }

    fn settings_window(&mut self, ctx: &Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new("Settings").open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).default_width(440.0).collapsible(false).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                ui.checkbox(&mut self.settings.vkb, "Use the built-in on-screen keyboard (off = Android keyboard)");
                ui.horizontal(|ui| {
                    ui.label("UI size");
                    ui.add(egui::Slider::new(&mut self.settings.ui_scale, 0.35..=1.5));
                });
                ui.horizontal(|ui| {
                    ui.label("Move snap");
                    ui.add(egui::DragValue::new(&mut self.settings.snap_move).speed(0.1).range(0.0..=64.0));
                    ui.label("Rotate snap");
                    ui.add(egui::DragValue::new(&mut self.settings.snap_rot).range(0.0..=90.0));
                    ui.label("Scale snap");
                    ui.add(egui::DragValue::new(&mut self.settings.snap_scale).speed(0.1).range(0.0..=64.0));
                });
                ui.separator();
                ui.strong("Roblox account");
                egui::Grid::new("setgrid").num_columns(2).show(ui, |ui| {
                    ui.label("OAuth client id");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.client_id).desired_width(260.0));
                    ui.end_row();
                    ui.label("Redirect URL");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.redirect).desired_width(260.0));
                    ui.end_row();
                    ui.label("Open Cloud API key");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.api_key).password(true).desired_width(260.0));
                    ui.end_row();
                    ui.label(".ROBLOSECURITY (optional)");
                    ui.add(egui::TextEdit::singleline(&mut self.settings.cookie).password(true).desired_width(260.0));
                    ui.end_row();
                });
                ui.weak("The cookie is only sent to Roblox asset endpoints to download places you can access, and is stored in this app's private folder. Never share it.");
                ui.separator();
                ui.label(format!("Data folder: {}", self.data_dir.display()));
                if ui.button("Save settings").clicked() {
                    self.save_settings();
                }
            });
        });
        if !open {
            self.save_settings();
        }
        self.show_settings = open;
    }

    // ---------------------------------------------------------------- on-screen keyboard
    fn vkb(&mut self, ctx: &Context) {
        const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
        const SYM: [&str; 4] = ["1234567890", "()[]{}<>=+-", "*/\\%^&|~#@", "\"'`;:,.?!_$"];
        let mut out: Vec<egui::Event> = Vec::new();
        let mut hide = false;
        let kb = self.kb_focus;
        egui::TopBottomPanel::bottom("vkb").show(ctx, |ui| {
            let w = ((ui.available_width() - 8.0) / 11.0).clamp(30.0, 70.0);
            let size = egui::vec2(w, 40.0);
            let text = |out: &mut Vec<egui::Event>, s: String| out.push(egui::Event::Text(s));
            let press = |out: &mut Vec<egui::Event>, key: egui::Key| {
                for pressed in [true, false] {
                    out.push(egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE });
                }
            };
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            if self.vkb_symbols {
                for row in SYM {
                    ui.horizontal(|ui| {
                        for ch in row.chars() {
                            if key_button(ui, &ch.to_string(), size) {
                                text(&mut out, ch.to_string());
                            }
                        }
                    });
                }
            } else {
                ui.horizontal(|ui| {
                    for ch in "1234567890".chars() {
                        if key_button(ui, &ch.to_string(), size) {
                            text(&mut out, ch.to_string());
                        }
                    }
                });
                for (i, row) in ROWS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if i == 2 && key_button(ui, if self.vkb_shift { "SHIFT" } else { "shift" }, egui::vec2(w * 1.5, 40.0)) {
                            self.vkb_shift = !self.vkb_shift;
                        }
                        for ch in row.chars() {
                            let c = if self.vkb_shift { ch.to_ascii_uppercase() } else { ch };
                            if key_button(ui, &c.to_string(), size) {
                                text(&mut out, c.to_string());
                                self.vkb_shift = false;
                            }
                        }
                        if i == 2 && key_button(ui, "<-", egui::vec2(w * 1.5, 40.0)) {
                            press(&mut out, egui::Key::Backspace);
                        }
                    });
                }
            }
            ui.horizontal(|ui| {
                if key_button(ui, if self.vkb_symbols { "abc" } else { "123#" }, egui::vec2(w * 1.5, 40.0)) {
                    self.vkb_symbols = !self.vkb_symbols;
                }
                if key_button(ui, "Tab", size) {
                    press(&mut out, egui::Key::Tab);
                }
                if key_button(ui, "space", egui::vec2(w * 3.0, 40.0)) {
                    text(&mut out, " ".into());
                }
                if key_button(ui, "<", size) {
                    press(&mut out, egui::Key::ArrowLeft);
                }
                if key_button(ui, ">", size) {
                    press(&mut out, egui::Key::ArrowRight);
                }
                if self.vkb_symbols && key_button(ui, "<-", size) {
                    press(&mut out, egui::Key::Backspace);
                }
                if key_button(ui, "Paste", egui::vec2(w * 1.3, 40.0)) {
                    out.push(egui::Event::Paste(self.clipboard.clone()));
                }
                if key_button(ui, "Enter", egui::vec2(w * 1.5, 40.0)) {
                    press(&mut out, egui::Key::Enter);
                }
                if key_button(ui, "Hide", egui::vec2(w * 1.2, 40.0)) {
                    hide = true;
                }
            });
        });
        if !out.is_empty() {
            self.inject.extend(out);
            if let Some(f) = kb {
                ctx.memory_mut(|m| m.request_focus(f));
            }
        }
        if hide {
            self.vkb_open = false;
            if let Some(f) = kb {
                ctx.memory_mut(|m| m.surrender_focus(f));
            }
        }
    }
}
