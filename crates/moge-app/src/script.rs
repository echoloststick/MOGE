//! Script editor: syntax colors, line numbers, diagnostics, autocomplete chips, find/replace, file loading.
use crate::studio::*;
use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Color32, FontId, Rect, Ui};
use moge_core::Value;
use moge_script::Kind;

fn kind_color(k: Kind) -> Color32 {
    match k {
        Kind::Keyword => Color32::from_rgb(197, 134, 192),
        Kind::Number => Color32::from_rgb(181, 206, 168),
        Kind::String => Color32::from_rgb(206, 145, 120),
        Kind::Comment => Color32::from_rgb(106, 153, 85),
        Kind::Builtin => Color32::from_rgb(86, 156, 214),
        Kind::Function => Color32::from_rgb(220, 220, 170),
        Kind::Property => Color32::from_rgb(156, 220, 254),
        Kind::TypeName => Color32::from_rgb(78, 201, 176),
        Kind::Punct | Kind::Operator => Color32::from_rgb(190, 190, 190),
        _ => Color32::from_rgb(212, 212, 212),
    }
}

fn hash_text(t: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn byte_of_char(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

impl Studio {
    pub fn script_ui(&mut self, ui: &mut Ui, rect: Rect) {
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("< 3D").clicked() {
                self.tab = Tab::View3d;
            }
            let cur = self.script.open.and_then(|i| self.ed.tree.name(i)).unwrap_or("(no script)").to_string();
            egui::ComboBox::from_id_salt("script_pick").selected_text(cur).width(180.0).show_ui(ui, |ui| {
                let list: Vec<moge_core::Id> = self.ed.tree.descendants(moge_core::Tree::ROOT).into_iter().filter(|i| self.is_script(*i)).take(500).collect();
                for i in list {
                    let n = self.ed.tree.full_name(i).unwrap_or_default();
                    if ui.selectable_label(self.script.open == Some(i), n).clicked() {
                        self.script.open = Some(i);
                    }
                }
            });
            if ui.button("Load file...").clicked() {
                self.open_dialog(FileAction::LoadIntoScript);
            }
            if ui.button("Import files...").clicked() {
                self.open_dialog(FileAction::ImportScripts);
            }
            if ui.button("Export...").clicked() {
                self.open_dialog(FileAction::ExportScript);
            }
            if ui.button("New").clicked() {
                self.insert.parent = None;
                self.insert_class("Script");
            }
            if ui.add_enabled(self.ed.can_undo(), egui::Button::new("Undo")).clicked() {
                self.ed.undo();
            }
            if ui.add_enabled(self.ed.can_redo(), egui::Button::new("Redo")).clicked() {
                self.ed.redo();
            }
        });
        let Some(id) = self.script.open.filter(|i| self.ed.tree.is_alive(*i) && self.is_script(*i)) else {
            ui.add_space(20.0);
            ui.label("No script is open. Pick one above, press 'Edit script' in the Explorer, or load a .lua / .luau / .txt file.");
            return;
        };
        let src = match self.ed.get_value(id, "Source") {
            Some(Value::String(s)) => s,
            _ => String::new(),
        };
        let mut text = src.clone();

        // find / replace
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.script.find).hint_text("Find").desired_width(120.0));
            ui.add(egui::TextEdit::singleline(&mut self.script.replace).hint_text("Replace").desired_width(120.0));
            let n = if self.script.find.is_empty() { 0 } else { text.matches(self.script.find.as_str()).count() };
            ui.label(format!("{n} match(es)"));
            if ui.add_enabled(n > 0, egui::Button::new("Replace all")).clicked() {
                text = text.replace(self.script.find.as_str(), self.script.replace.as_str());
            }
        });

        // cached highlighting / diagnostics
        let key = hash_text(&text);
        if self.script.diag_key != key {
            self.script.diags = moge_script::check(&text);
            self.script.diag_key = key;
        }
        let bottom_h = 96.0;
        let avail = ui.available_size();
        let mut font = FontId::monospace(15.0);
        font.size = 15.0;
        let mut spans = std::mem::take(&mut self.script.spans);
        let mut span_key = self.script.span_key;
        let lines = text.bytes().filter(|b| *b == b'\n').count() + 1;
        let mut out_id: Option<egui::Id> = None;
        let mut cursor_char: Option<usize> = None;
        let mut focused = false;
        egui::ScrollArea::both().id_salt("script_scroll").auto_shrink([false, false]).max_height((avail.y - bottom_h).max(120.0)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                if lines < 20000 {
                    let nums: String = (1..=lines).map(|n| n.to_string()).collect::<Vec<_>>().join("\n");
                    ui.add(egui::Label::new(egui::RichText::new(nums).font(font.clone()).color(Color32::from_gray(110))).selectable(false));
                }
                let f2 = font.clone();
                let mut layouter = |_ui: &Ui, s: &str, wrap: f32| {
                    let k = hash_text(s);
                    if k != span_key {
                        spans = moge_script::colorize(s);
                        span_key = k;
                    }
                    let mut job = LayoutJob::default();
                    job.wrap.max_width = wrap;
                    let fmt = |c: Color32| TextFormat { font_id: f2.clone(), color: c, ..Default::default() };
                    let mut pos = 0usize;
                    for sp in &spans {
                        if sp.start < pos || sp.end > s.len() || sp.start >= sp.end {
                            continue;
                        }
                        if sp.start > pos {
                            if let Some(t) = s.get(pos..sp.start) {
                                job.append(t, 0.0, fmt(kind_color(Kind::Ident)));
                            }
                        }
                        if let Some(t) = s.get(sp.start..sp.end) {
                            job.append(t, 0.0, fmt(kind_color(sp.kind)));
                            pos = sp.end;
                        }
                    }
                    if let Some(t) = s.get(pos..) {
                        if !t.is_empty() {
                            job.append(t, 0.0, fmt(kind_color(Kind::Ident)));
                        }
                    }
                    ui_fonts_layout(&_ui.ctx().clone(), job)
                };
                let output = egui::TextEdit::multiline(&mut text)
                    .font(font.clone())
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
                    .min_size(egui::vec2((rect.width() - 60.0).max(100.0), (avail.y - bottom_h - 10.0).max(80.0)))
                    .layouter(&mut layouter)
                    .show(ui);
                out_id = Some(output.response.id);
                focused = output.response.has_focus();
                cursor_char = output.cursor_range.map(|c| c.primary.ccursor.index);
            });
        });
        self.script.spans = spans;
        self.script.span_key = span_key;

        // completion cache: refreshed while the editor has focus, kept while a chip is being tapped
        if focused {
            if let Some(ci) = cursor_char {
                let b = byte_of_char(&text, ci);
                let c = moge_script::complete(&text, b, &self.ed.dump);
                self.script.comps = c.items.into_iter().take(24).collect();
                self.script.comp_from = c.replace_from;
                self.script.comp_cursor = b;
            }
        }
        let mut applied: Option<usize> = None;
        let mut goto: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            let comps = self.script.comps.clone();
            for c in comps {
                if ui.small_button(&c.label).on_hover_text(&c.detail).clicked() {
                    let (from, to) = (self.script.comp_from, self.script.comp_cursor);
                    if from <= to && to <= text.len() && text.is_char_boundary(from) && text.is_char_boundary(to) {
                        text.replace_range(from..to, &c.label);
                        applied = Some(text[..from].chars().count() + c.label.chars().count());
                        self.script.comps.clear();
                    }
                }
            }
        });
        egui::ScrollArea::vertical().id_salt("diag_scroll").max_height(bottom_h - 30.0).auto_shrink([false, true]).show(ui, |ui| {
            if self.script.diags.is_empty() {
                ui.colored_label(Color32::from_rgb(110, 190, 120), "No syntax problems found");
            }
            for d in self.script.diags.iter().take(30) {
                if ui.add(egui::Label::new(egui::RichText::new(format!("{}:{}  {}", d.line, d.col, d.message)).color(Color32::from_rgb(240, 120, 120))).sense(egui::Sense::click())).clicked() {
                    goto = Some(src_chars_before(&text, d.start));
                }
            }
        });
        if let (Some(id_text), Some(ci)) = (out_id, applied.or(goto)) {
            if let Some(mut st) = egui::TextEdit::load_state(&ctx, id_text) {
                st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(ci))));
                st.store(&ctx, id_text);
            }
            ctx.memory_mut(|m| m.request_focus(id_text));
        }
        if text != src {
            if let Err(e) = self.ed.live_edit(|ed| ed.set_value(&[id], "Source", Value::String(text))) {
                self.log(Level::Warn, format!("Cannot edit the script: {e}"));
            }
        }
    }
}

fn src_chars_before(text: &str, byte: usize) -> usize {
    let b = byte.min(text.len());
    text.get(..b).map(|t| t.chars().count()).unwrap_or(0)
}

fn ui_fonts_layout(ctx: &egui::Context, job: LayoutJob) -> std::sync::Arc<egui::Galley> {
    ctx.fonts(|f| f.layout_job(job))
}
