//! Application state and non-visual actions (projects, imports, account, background jobs).
use crate::net::{self, Msg, Net, UreqHttp};
use egui::Context;
use moge_core::editor::Editor;
use moge_core::fs_browser::{self, Entry};
use moge_core::{project, script_file, template, ApiDump, Id, Tree, Value};
use moge_gui::Mode;
use moge_math::OrbitCamera;
use moge_roblox::{Game, Token, User};
use moge_scene::{tools::Tool, Scene};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    View3d,
    Gui,
    Script,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Explorer,
    Toolbox,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Right {
    Properties,
    Output,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    OpenProject,
    ImportScripts,
    ImportModel,
    LoadIntoScript,
    SaveProject,
    ExportRbxlx,
    ExportScript,
}

impl FileAction {
    pub fn title(self) -> &'static str {
        match self {
            FileAction::OpenProject => "Open project / place",
            FileAction::ImportScripts => "Import scripts (.lua .luau .txt)",
            FileAction::ImportModel => "Import model / place file",
            FileAction::LoadIntoScript => "Load file into the open script",
            FileAction::SaveProject => "Save project",
            FileAction::ExportRbxlx => "Export .rbxlx",
            FileAction::ExportScript => "Export script as .luau",
        }
    }
    pub fn exts(self) -> &'static [&'static str] {
        match self {
            FileAction::OpenProject => &["moge", "rbxl", "rbxlx", "rbxm", "rbxmx"],
            FileAction::ImportScripts | FileAction::LoadIntoScript => script_file::EXTENSIONS,
            FileAction::ImportModel => &["rbxm", "rbxmx", "rbxl", "rbxlx"],
            FileAction::SaveProject => &["moge"],
            FileAction::ExportRbxlx => &["rbxlx"],
            FileAction::ExportScript => &["luau", "lua"],
        }
    }
    pub fn is_save(self) -> bool {
        matches!(self, FileAction::SaveProject | FileAction::ExportRbxlx | FileAction::ExportScript)
    }
    pub fn multi(self) -> bool {
        matches!(self, FileAction::ImportScripts)
    }
}

pub struct FileDlg {
    pub action: FileAction,
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    pub name: String,
    pub selected: Vec<PathBuf>,
}

pub enum Img {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

#[derive(Clone, Default)]
pub struct Settings {
    pub client_id: String,
    pub redirect: String,
    pub cookie: String,
    pub api_key: String,
    pub universe: String,
    pub place: String,
    pub vkb: bool,
    pub snap_move: f32,
    pub snap_rot: f32,
    pub snap_scale: f32,
    /// UI size multiplier on top of the screen density (0 = default for the platform).
    pub ui_scale: f32,
}

impl Settings {
    pub fn defaults() -> Settings {
        Settings { redirect: "http://localhost:3000/callback".into(), vkb: false, snap_move: 1.0, snap_rot: 15.0, snap_scale: 0.5, ui_scale: if cfg!(target_os = "android") { 0.6 } else { 1.0 }, ..Default::default() }
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::defaults();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "client_id" => s.client_id = v.into(),
                "redirect" => s.redirect = v.into(),
                "cookie" => s.cookie = v.into(),
                "api_key" => s.api_key = v.into(),
                "universe" => s.universe = v.into(),
                "place" => s.place = v.into(),
                "vkb_custom" => s.vkb = v == "1",
                "snap_move" => s.snap_move = v.parse().unwrap_or(s.snap_move),
                "snap_rot" => s.snap_rot = v.parse().unwrap_or(s.snap_rot),
                "snap_scale" => s.snap_scale = v.parse().unwrap_or(s.snap_scale),
                "ui_scale" => s.ui_scale = v.parse().unwrap_or(s.ui_scale),
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        let clean = |s: &str| s.replace(['\n', '\r'], " ");
        format!(
            "client_id={}\nredirect={}\ncookie={}\napi_key={}\nuniverse={}\nplace={}\nvkb_custom={}\nsnap_move={}\nsnap_rot={}\nsnap_scale={}\nui_scale={}\n",
            clean(&self.client_id),
            clean(&self.redirect),
            clean(&self.cookie),
            clean(&self.api_key),
            clean(&self.universe),
            clean(&self.place),
            u8::from(self.vkb),
            self.snap_move,
            self.snap_rot,
            self.snap_scale,
            self.ui_scale
        )
    }
}

#[derive(Default)]
pub struct Account {
    pub verifier: String,
    pub state: String,
    pub url: String,
    pub pasted: String,
    pub token: Option<Token>,
    pub user: Option<User>,
    pub games: Vec<Game>,
    pub use_api_key: bool,
    pub status: String,
    pub busy: bool,
}

pub struct ToolboxState {
    pub id_text: String,
    pub as_model: bool,
    pub status: String,
    pub recent: Vec<String>,
}

pub struct InsertState {
    pub open: bool,
    pub query: String,
    pub parent: Option<Id>,
}

pub struct ScriptState {
    pub open: Option<Id>,
    pub find: String,
    pub replace: String,
    pub span_key: u64,
    pub spans: Vec<moge_script::Span>,
    pub diag_key: u64,
    pub diags: Vec<moge_script::Diagnostic>,
    pub comps: Vec<moge_script::Completion>,
    pub comp_from: usize,
    pub comp_cursor: usize,
}

pub enum GuiDrag {
    Move { id: Id, pos0: (f32, f32, f32, f32), parent: moge_gui::Rect, rect0: moge_gui::Rect, total: (f32, f32) },
    Resize { id: Id, handle: (i8, i8), pos0: (f32, f32, f32, f32), size0: (f32, f32, f32, f32), anchor: [f32; 2], parent: moge_gui::Rect, current: (f32, f32), total: (f32, f32) },
    Pan,
}

pub struct GuiState {
    pub root: Option<Id>,
    pub device: usize,
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub mode: Mode,
    pub drag: Option<GuiDrag>,
    pub guides: Vec<moge_gui::Guide>,
    pub snap: bool,
    pub fit_pending: bool,
}

pub enum Drag3d {
    Orbit,
    Handle { axis: usize, total: (f32, f32), origin: [f32; 3], axes: [[f32; 3]; 3], parts: Vec<(Id, [f32; 3], [f32; 9], [f32; 3])> },
}

pub struct Studio {
    pub ed: Editor,
    pub dump_ready: bool,
    pub data_dir: PathBuf,
    pub start_dirs: Vec<(String, PathBuf)>,
    pub settings: Settings,
    pub project_name: String,
    pub project_path: Option<PathBuf>,
    pub cam: OrbitCamera,
    pub scene: Scene,
    pub scene_rev: u64,
    pub viewport_px: [f32; 4],
    pub tool: Tool,
    pub drag3d: Option<Drag3d>,
    pub multi: bool,
    pub tab: Tab,
    pub side: Side,
    pub right: Right,
    pub show_left: bool,
    pub show_right: bool,
    pub open_nodes: HashSet<Id>,
    pub explorer_filter: String,
    pub prop_filter: String,
    pub move_src: Option<Vec<Id>>,
    pub rename: Option<(Id, String)>,
    pub insert: InsertState,
    pub toolbox: ToolboxState,
    pub files: Option<FileDlg>,
    pub account: Account,
    pub show_account: bool,
    pub show_settings: bool,
    pub script: ScriptState,
    pub gui: GuiState,
    pub images: HashMap<String, Img>,
    pub output: Vec<(Level, String)>,
    pub run_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub net: Net,
    pub http: Arc<UreqHttp>,
    pub inject: Vec<egui::Event>,
    pub vkb_open: bool,
    pub vkb_shift: bool,
    pub vkb_symbols: bool,
    pub kb_focus: Option<egui::Id>,
    pub paste_req: bool,
    pub kb_last: Option<egui::Id>,
    pub clipboard: String,
    pub confirm_new: bool,
    last_autosave: Instant,
}

pub const DEVICES: &[(&str, f32, f32)] = &[("Phone landscape", 844.0, 390.0), ("Phone portrait", 390.0, 844.0), ("Tablet", 1024.0, 768.0), ("Desktop", 1920.0, 1080.0)];

fn empty_dump() -> Arc<ApiDump> {
    Arc::new(ApiDump::from_json(r#"{"Classes":[],"Enums":[]}"#).unwrap_or_default())
}

fn random_bytes(n: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    let mut out = Vec::new();
    while out.len() < n {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        h.write_u128(nanos);
        h.write_usize(out.len());
        out.extend_from_slice(&h.finish().to_le_bytes());
    }
    out.truncate(n);
    out
}

impl Studio {
    pub fn new(data_dir: PathBuf, start_dirs: Vec<(String, PathBuf)>) -> Studio {
        let _ = std::fs::create_dir_all(&data_dir);
        let settings = std::fs::read_to_string(data_dir.join("settings.txt")).map(|t| Settings::parse(&t)).unwrap_or_else(|_| Settings::defaults());
        let mut s = Studio {
            ed: Editor::new(template::baseplate(), empty_dump()),
            dump_ready: false,
            data_dir,
            start_dirs,
            settings,
            project_name: "Untitled".into(),
            project_path: None,
            cam: OrbitCamera { target: [0.0, 4.0, 0.0], yaw: 0.6, pitch: 0.45, dist: 40.0 },
            scene: Scene::default(),
            scene_rev: u64::MAX,
            viewport_px: [0.0, 0.0, 0.0, 0.0],
            tool: Tool::Select,
            drag3d: None,
            multi: false,
            tab: Tab::View3d,
            side: Side::Explorer,
            right: Right::Properties,
            show_left: true,
            show_right: true,
            open_nodes: HashSet::new(),
            explorer_filter: String::new(),
            prop_filter: String::new(),
            move_src: None,
            rename: None,
            insert: InsertState { open: false, query: String::new(), parent: None },
            toolbox: ToolboxState { id_text: String::new(), as_model: true, status: String::new(), recent: Vec::new() },
            files: None,
            account: Account::default(),
            show_account: false,
            show_settings: false,
            script: ScriptState { open: None, find: String::new(), replace: String::new(), span_key: u64::MAX, spans: Vec::new(), diag_key: u64::MAX, diags: Vec::new(), comps: Vec::new(), comp_from: 0, comp_cursor: 0 },
            gui: GuiState { root: None, device: 0, zoom: 1.0, pan: egui::Vec2::ZERO, mode: Mode::Offset, drag: None, guides: Vec::new(), snap: true, fit_pending: true },
            images: HashMap::new(),
            output: Vec::new(),
            run_cancel: None,
            net: Net::new(),
            http: Arc::new(UreqHttp::new()),
            inject: Vec::new(),
            vkb_open: false,
            vkb_shift: false,
            vkb_symbols: false,
            kb_focus: None,
            paste_req: false,
            kb_last: None,
            clipboard: String::new(),
            confirm_new: false,
            last_autosave: Instant::now(),
        };
        s.open_nodes.insert(Tree::ROOT);
        s.load_dump();
        s.log(Level::Info, if cfg!(feature = "luau") { "Luau runtime (mlua): included in this build" } else { "Luau runtime: NOT included in this build (the CI Luau build failed and fell back)" });
        s
    }

    pub fn log(&mut self, level: Level, msg: impl Into<String>) {
        let msg = msg.into();
        match level {
            Level::Info => log::info!("{msg}"),
            Level::Warn => log::warn!("{msg}"),
            Level::Error => log::error!("{msg}"),
        }
        self.output.push((level, msg));
        if self.output.len() > 500 {
            self.output.drain(0..100);
        }
    }

    // ---------------------------------------------------------------- API dump
    fn set_dump(&mut self, text: &str) -> Result<usize, String> {
        let d = ApiDump::from_json(text)?;
        let n = d.class_count();
        if n == 0 {
            return Err("The API dump contains no classes".into());
        }
        self.ed.dump = Arc::new(d);
        self.ed.mark_changed();
        self.dump_ready = true;
        Ok(n)
    }

    fn load_dump(&mut self) {
        let embedded = include_str!(concat!(env!("OUT_DIR"), "/embedded_dump.json"));
        if !embedded.is_empty() {
            match self.set_dump(embedded) {
                Ok(n) => return self.log(Level::Info, format!("Loaded the embedded API dump ({n} classes)")),
                Err(e) => self.log(Level::Warn, format!("Embedded API dump rejected: {e}")),
            }
        }
        if let Ok(text) = std::fs::read_to_string(self.data_dir.join("api_dump.json")) {
            match self.set_dump(&text) {
                Ok(n) => return self.log(Level::Info, format!("Loaded the cached API dump ({n} classes)")),
                Err(e) => self.log(Level::Warn, format!("Cached API dump rejected: {e}")),
            }
        }
        self.download_dump();
    }

    pub fn download_dump(&mut self) {
        self.log(Level::Info, "Downloading the Roblox API dump...");
        let http = self.http.clone();
        self.net.spawn(move || Msg::Dump(net::download_text(&*http, net::DUMP_URL)));
    }

    // ---------------------------------------------------------------- scene
    /// Rebuilds the render scene when the tree changed. Returns true when the GPU copy must be refreshed.
    pub fn refresh_scene(&mut self) -> bool {
        if self.scene_rev == self.ed.revision {
            return false;
        }
        self.scene = moge_scene::build(&self.ed.tree);
        self.scene_rev = self.ed.revision;
        true
    }

    pub fn focus_selection(&mut self) {
        let ids = self.ed.selection.clone();
        if let Some((c, r)) = self.scene.bounds(&self.ed.tree, &ids) {
            self.cam.focus(c, r.max(1.0));
        }
    }

    // ---------------------------------------------------------------- projects
    pub fn new_project(&mut self) {
        self.ed.replace_tree(template::baseplate());
        self.after_load("Untitled".into(), None);
    }

    fn after_load(&mut self, name: String, path: Option<PathBuf>) {
        self.project_name = name;
        self.project_path = path;
        self.ed.dirty = false;
        self.ed.mark_changed();
        self.scene_rev = u64::MAX;
        self.script.open = None;
        self.gui.root = None;
        self.gui.fit_pending = true;
        self.open_nodes.clear();
        self.open_nodes.insert(Tree::ROOT);
        self.cam = OrbitCamera { target: [0.0, 4.0, 0.0], yaw: 0.6, pitch: 0.45, dist: 40.0 };
    }

    pub fn open_file(&mut self, path: &Path) {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled").to_string();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => return self.log(Level::Error, format!("Failed to read {}: {e}", path.display())),
        };
        match ext.as_str() {
            "moge" => match String::from_utf8(bytes).map_err(|_| "The project file is not valid UTF-8".to_string()).and_then(|t| project::load(&t)) {
                Ok(l) => {
                    self.ed.replace_tree(l.tree);
                    self.after_load(l.name, Some(path.to_path_buf()));
                    self.log(Level::Info, format!("Opened project {}", path.display()));
                }
                Err(e) => self.log(Level::Error, format!("Failed to open the project: {e}")),
            },
            "rbxl" | "rbxlx" | "rbxm" | "rbxmx" => {
                let mut t = Tree::new();
                match moge_formats::load(&bytes, &mut t) {
                    Ok(imp) => {
                        for w in imp.warnings.iter().take(20) {
                            self.log(Level::Warn, format!("Import warning: {w}"));
                        }
                        t.merge_place(&self.ed.dump, &imp.roots);
                        self.ed.replace_tree(t);
                        self.after_load(name, None);
                        self.log(Level::Info, format!("Imported {}", path.display()));
                    }
                    Err(e) => self.log(Level::Error, format!("Failed to import {}: {e}", path.display())),
                }
            }
            _ => self.log(Level::Error, format!("Unsupported file type: .{ext}")),
        }
    }

    pub fn save_project(&mut self, path: &Path) {
        let text = project::save(&self.ed.tree, &self.project_name);
        match std::fs::write(path, text) {
            Ok(()) => {
                self.ed.dirty = false;
                self.project_path = Some(path.to_path_buf());
                self.log(Level::Info, format!("Saved {}", path.display()));
            }
            Err(e) => self.log(Level::Error, format!("Failed to save {}: {e}", path.display())),
        }
    }

    pub fn export_rbxlx(&mut self, path: &Path) {
        let text = moge_formats::write_rbxlx(&self.ed.tree, &self.ed.dump);
        match std::fs::write(path, text) {
            Ok(()) => self.log(Level::Info, format!("Exported {}", path.display())),
            Err(e) => self.log(Level::Error, format!("Failed to export {}: {e}", path.display())),
        }
    }

    pub fn autosave(&mut self) {
        if self.ed.dirty && self.last_autosave.elapsed().as_secs() >= 60 {
            self.last_autosave = Instant::now();
            let text = project::save(&self.ed.tree, &self.project_name);
            if let Err(e) = std::fs::write(self.data_dir.join("autosave.moge"), text) {
                self.log(Level::Warn, format!("Autosave failed: {e}"));
            }
        }
    }

    pub fn restore_autosave(&mut self) {
        let p = self.data_dir.join("autosave.moge");
        if p.exists() {
            self.open_file(&p);
            self.project_path = None;
            self.ed.dirty = true;
        } else {
            self.log(Level::Warn, "There is no autosave to restore");
        }
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = std::fs::write(self.data_dir.join("settings.txt"), self.settings.serialize()) {
            self.log(Level::Warn, format!("Failed to save settings: {e}"));
        }
    }

    // ---------------------------------------------------------------- file dialog
    pub fn open_dialog(&mut self, action: FileAction) {
        let dir = self.files.as_ref().map(|f| f.dir.clone()).or_else(|| self.start_dirs.first().map(|d| d.1.clone())).unwrap_or_else(|| PathBuf::from("."));
        let name = match action {
            FileAction::SaveProject => format!("{}.moge", self.project_name),
            FileAction::ExportRbxlx => format!("{}.rbxlx", self.project_name),
            FileAction::ExportScript => {
                let n = self.script.open.and_then(|i| self.ed.tree.name(i)).unwrap_or("Script");
                format!("{n}.luau")
            }
            _ => String::new(),
        };
        self.files = Some(FileDlg { action, dir: dir.clone(), entries: Vec::new(), error: None, name, selected: Vec::new() });
        self.navigate(dir);
    }

    pub fn navigate(&mut self, dir: PathBuf) {
        let Some(f) = self.files.as_mut() else { return };
        match fs_browser::list_dir(&dir, f.action.exts()) {
            Ok(e) => {
                f.entries = e;
                f.error = None;
                f.dir = dir;
            }
            Err(e) => f.error = Some(format!("{e}. On Android 11+ grant 'All files access' to MOGE in the system settings, or use the app folder.")),
        }
    }

    /// Runs the dialog's action on a chosen file (or the typed name for save dialogs).
    pub fn dialog_confirm(&mut self, path: PathBuf) {
        let Some(action) = self.files.as_ref().map(|f| f.action) else { return };
        match action {
            FileAction::OpenProject => self.open_file(&path),
            FileAction::ImportScripts => {
                let list = self.files.as_ref().map(|f| f.selected.clone()).unwrap_or_default();
                let list = if list.is_empty() { vec![path] } else { list };
                self.import_scripts(&list);
            }
            FileAction::ImportModel => self.import_model_file(&path),
            FileAction::LoadIntoScript => self.load_into_script(&path),
            FileAction::SaveProject => self.save_project(&path),
            FileAction::ExportRbxlx => self.export_rbxlx(&path),
            FileAction::ExportScript => {
                let src = self.script.open.and_then(|i| self.ed.get_value(i, "Source"));
                match src {
                    Some(Value::String(s)) => match std::fs::write(&path, s) {
                        Ok(()) => self.log(Level::Info, format!("Exported {}", path.display())),
                        Err(e) => self.log(Level::Error, format!("Failed to export {}: {e}", path.display())),
                    },
                    _ => self.log(Level::Warn, "Open a script first"),
                }
            }
        }
        self.files = None;
    }

    // ---------------------------------------------------------------- scripts
    fn default_parent(&self, class: &str) -> Id {
        let sel = self.ed.selection.first().copied().filter(|i| self.ed.tree.is_alive(*i) && *i != Tree::ROOT);
        if let Some(s) = sel {
            return s;
        }
        let t = &self.ed.tree;
        let svc = match class {
            "ModuleScript" => "ReplicatedStorage",
            "LocalScript" => "StarterPlayer",
            _ => "ServerScriptService",
        };
        t.find_first_child_of_class(Tree::ROOT, svc).or_else(|| t.find_first_child_of_class(Tree::ROOT, "Workspace")).unwrap_or(Tree::ROOT)
    }

    pub fn import_scripts(&mut self, paths: &[PathBuf]) {
        let mut last = None;
        let mut ok = 0;
        for p in paths {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("Script").to_string();
            let bytes = match std::fs::read(p) {
                Ok(b) => b,
                Err(e) => {
                    self.log(Level::Error, format!("Failed to read {}: {e}", p.display()));
                    continue;
                }
            };
            match script_file::parse(&name, &bytes, None) {
                Ok(f) => {
                    let parent = self.default_parent(f.class);
                    match self.ed.import_script(&f, parent) {
                        Ok(id) => {
                            ok += 1;
                            last = Some(id);
                            self.open_nodes.insert(parent);
                        }
                        Err(e) => self.log(Level::Error, format!("Failed to import {name}: {e}")),
                    }
                }
                Err(e) => self.log(Level::Error, format!("{name}: {e}")),
            }
        }
        if let Some(id) = last {
            self.open_script(id);
        }
        self.log(Level::Info, format!("Imported {ok} of {} script file(s)", paths.len()));
    }

    pub fn load_into_script(&mut self, path: &Path) {
        let Some(id) = self.script.open else { return self.log(Level::Warn, "Open a script first") };
        match std::fs::read(path) {
            Ok(b) => match self.ed.load_source(id, &b) {
                Ok(()) => self.log(Level::Info, format!("Loaded {} into the script", path.display())),
                Err(e) => self.log(Level::Error, format!("Failed to load the script: {e}")),
            },
            Err(e) => self.log(Level::Error, format!("Failed to read {}: {e}", path.display())),
        }
    }

    pub fn open_script(&mut self, id: Id) {
        self.script.open = Some(id);
        self.tab = Tab::Script;
    }

    pub fn is_script(&self, id: Id) -> bool {
        self.ed.tree.node(id).map(|n| matches!(n.class.as_str(), "Script" | "LocalScript" | "ModuleScript")).unwrap_or(false)
    }

    // ---------------------------------------------------------------- imports
    fn import_model_file(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(b) => self.apply_import(&path.display().to_string(), &b, true),
            Err(e) => self.log(Level::Error, format!("Failed to read {}: {e}", path.display())),
        }
    }

    /// Loads rbxl/rbxlx/rbxm bytes into the open project. Models go under the selection, places merge services.
    pub fn apply_import(&mut self, label: &str, bytes: &[u8], as_model: bool) {
        let parent = self.ed.selection.first().copied().filter(|i| self.ed.tree.is_alive(*i) && *i != Tree::ROOT).or_else(|| self.ed.tree.find_first_child_of_class(Tree::ROOT, "Workspace")).unwrap_or(Tree::ROOT);
        match moge_formats::load(bytes, &mut self.ed.tree) {
            Ok(imp) => {
                for w in imp.warnings.iter().take(20) {
                    self.log(Level::Warn, format!("Import warning: {w}"));
                }
                let dump = self.ed.dump.clone();
                let is_place = imp.roots.iter().any(|r| self.ed.tree.node(*r).map(|n| n.class == "DataModel" || dump.class(&n.class).map(|c| c.service).unwrap_or(false)).unwrap_or(false));
                if is_place || !as_model {
                    let n = self.ed.tree.merge_place(&dump, &imp.roots).len();
                    self.log(Level::Info, format!("Merged {n} top-level object(s) from {label}"));
                } else {
                    let mut moved = Vec::new();
                    for r in &imp.roots {
                        if self.ed.tree.set_parent(*r, Some(parent)).is_ok() {
                            moved.push(*r);
                        }
                    }
                    self.ed.select(moved.clone());
                    self.open_nodes.insert(parent);
                    self.log(Level::Info, format!("Inserted {} object(s) from {label}", moved.len()));
                }
                self.ed.dirty = true;
                self.ed.mark_changed();
            }
            Err(e) => self.log(Level::Error, format!("Failed to import {label}: {e}")),
        }
    }

    pub fn import_asset_id(&mut self) {
        let text = self.toolbox.id_text.trim().to_string();
        let Ok(id) = text.trim_start_matches("rbxassetid://").parse::<u64>() else {
            self.toolbox.status = "Enter a numeric place or asset id".into();
            return;
        };
        let cookie = if self.settings.cookie.trim().is_empty() { None } else { Some(self.settings.cookie.trim().to_string()) };
        let http = self.http.clone();
        let as_model = self.toolbox.as_model;
        self.toolbox.status = format!("Downloading {id}...");
        if !self.toolbox.recent.contains(&text) {
            self.toolbox.recent.insert(0, text);
            self.toolbox.recent.truncate(8);
        }
        self.net.spawn(move || Msg::Asset { label: id.to_string(), as_model, data: moge_roblox::fetch_asset(&*http, id, cookie.as_deref()) });
    }

    // ---------------------------------------------------------------- images
    pub fn request_image(&mut self, key: &str) {
        if key.is_empty() || self.images.contains_key(key) {
            return;
        }
        self.images.insert(key.to_string(), Img::Loading);
        let (http, k) = (self.http.clone(), key.to_string());
        self.net.spawn(move || {
            let image = net::fetch_image(&*http, &k);
            Msg::Image { key: k, image }
        });
    }

    // ---------------------------------------------------------------- account
    pub fn begin_login(&mut self) {
        if self.settings.client_id.trim().is_empty() {
            self.account.status = "Set your Roblox OAuth client id in Settings first (Creator Hub > OAuth 2.0 Apps)".into();
            return;
        }
        let verifier = match moge_roblox::pkce_verifier(&random_bytes(48)) {
            Ok(v) => v,
            Err(e) => {
                self.account.status = e;
                return;
            }
        };
        let state = moge_roblox::base64url(&random_bytes(16));
        let url = moge_roblox::authorize_url(self.settings.client_id.trim(), self.settings.redirect.trim(), &state, &moge_roblox::pkce_challenge(&verifier));
        self.account.verifier = verifier;
        self.account.state = state;
        self.account.url = url.clone();
        self.account.status = match webbrowser::open(&url) {
            Ok(()) => "Sign in in the browser, then paste the full address of the page it ends on.".into(),
            Err(e) => format!("Could not open the browser ({e}). Copy the link below into a browser manually."),
        };
    }

    pub fn finish_login(&mut self) {
        let code = match moge_roblox::parse_redirect(self.account.pasted.trim(), &self.account.state) {
            Ok(c) => c,
            Err(e) => {
                self.account.status = e;
                return;
            }
        };
        let (http, cid, redirect, verifier) = (self.http.clone(), self.settings.client_id.trim().to_string(), self.settings.redirect.trim().to_string(), self.account.verifier.clone());
        self.account.busy = true;
        self.account.status = "Signing in...".into();
        self.net.spawn(move || {
            let r = (|| -> Result<(Token, User), String> {
                let tok = moge_roblox::parse_token(&moge_roblox::Http::send(&*http, &moge_roblox::token_request(&cid, None, &code, &verifier, &redirect))?)?;
                let user = moge_roblox::parse_user(&moge_roblox::Http::send(&*http, &moge_roblox::userinfo_request(&tok.access))?)?;
                Ok((tok, user))
            })();
            Msg::Login(r)
        });
    }

    pub fn logout(&mut self) {
        self.account = Account::default();
    }

    pub fn publish(&mut self) {
        let (Ok(universe), Ok(place)) = (self.settings.universe.trim().parse::<u64>(), self.settings.place.trim().parse::<u64>()) else {
            self.account.status = "Enter a numeric Universe id and Place id".into();
            return;
        };
        let credential = if self.account.use_api_key { self.settings.api_key.trim().to_string() } else { self.account.token.as_ref().map(|t| t.access.clone()).unwrap_or_default() };
        if credential.is_empty() {
            self.account.status = if self.account.use_api_key { "Set an Open Cloud API key in Settings".into() } else { "Sign in first".into() };
            return;
        }
        let body = moge_formats::write_rbxlx(&self.ed.tree, &self.ed.dump).into_bytes();
        let mut req = moge_roblox::publish_request(universe, place, body, false, &credential);
        if self.account.use_api_key {
            req.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
            req.headers.push(("x-api-key".into(), credential));
        }
        let http = self.http.clone();
        self.account.busy = true;
        self.account.status = "Publishing...".into();
        self.net.spawn(move || Msg::Published(moge_roblox::Http::send(&*http, &req).and_then(|r| moge_roblox::parse_publish(&r))));
    }

    pub fn fetch_games(&mut self) {
        let Some(u) = self.account.user.as_ref().map(|u| u.id) else { return };
        let http = self.http.clone();
        self.net.spawn(move || Msg::Games(moge_roblox::Http::send(&*http, &moge_roblox::user_games_request(u)).and_then(|r| moge_roblox::parse_games(&r))));
    }

    // ---------------------------------------------------------------- background results
    pub fn poll_net(&mut self, ctx: &Context) {
        while let Ok(msg) = self.net.rx.try_recv() {
            match msg {
                Msg::Dump(Ok(text)) => match self.set_dump(&text) {
                    Ok(n) => {
                        let _ = std::fs::write(self.data_dir.join("api_dump.json"), &text);
                        self.log(Level::Info, format!("API dump ready ({n} classes)"));
                    }
                    Err(e) => self.log(Level::Error, format!("The downloaded API dump is invalid: {e}")),
                },
                Msg::Dump(Err(e)) => self.log(Level::Error, format!("Failed to download the API dump: {e}. Check the connection and retry from File > Update API dump.")),
                Msg::Asset { label, as_model, data } => match data {
                    Ok(bytes) => {
                        self.toolbox.status = format!("Downloaded {} KB", bytes.len() / 1024);
                        self.apply_import(&format!("asset {label}"), &bytes, as_model);
                    }
                    Err(e) => {
                        self.toolbox.status = "Download failed".into();
                        self.log(Level::Error, format!("Failed to download asset {label}: {e}"));
                    }
                },
                Msg::Login(Ok((tok, user))) => {
                    self.account.status = format!("Signed in as {}", user.name);
                    self.account.token = Some(tok);
                    self.account.user = Some(user);
                    self.account.busy = false;
                    self.fetch_games();
                }
                Msg::Login(Err(e)) => {
                    self.account.busy = false;
                    self.account.status = format!("Sign-in failed: {e}");
                }
                Msg::Games(Ok(g)) => self.account.games = g,
                Msg::Games(Err(e)) => self.log(Level::Warn, format!("Could not list your games: {e}")),
                Msg::Published(r) => {
                    self.account.busy = false;
                    match r {
                        Ok(v) => {
                            self.account.status = format!("Published as version {v}");
                            self.log(Level::Info, format!("Published place as version {v}"));
                        }
                        Err(e) => {
                            self.account.status = format!("Publish failed: {e}");
                            self.log(Level::Error, format!("Publish failed: {e}"));
                        }
                    }
                }
                Msg::RunLine(l, m) => self.log(l, m),
                Msg::RunDone => {
                    self.run_cancel = None;
                    self.log(Level::Info, "Run finished");
                }
                Msg::Image { key, image } => match image {
                    Ok(img) => {
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
                        let tex = ctx.load_texture(format!("img:{key}"), ci, egui::TextureOptions::LINEAR);
                        self.images.insert(key, Img::Ready(tex));
                    }
                    Err(e) => {
                        self.log(Level::Warn, format!("Image {key}: {e}"));
                        self.images.insert(key, Img::Failed);
                    }
                },
            }
        }
    }
}
