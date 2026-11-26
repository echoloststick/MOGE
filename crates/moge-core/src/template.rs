//! Starter project and per-class insert defaults.
use crate::tree::Tree;
use crate::value::{Id, Value, IDENTITY_ROT};

pub const MATERIAL_PLASTIC: u32 = 256;
pub const SHAPE_BLOCK: u32 = 1;

pub fn cframe_at(pos: [f32; 3]) -> Value {
    Value::CFrame { pos, rot: IDENTITY_ROT }
}

pub fn is_base_part(class: &str) -> bool {
    matches!(class, "Part" | "WedgePart" | "CornerWedgePart" | "SpawnLocation" | "MeshPart" | "UnionOperation" | "TrussPart" | "Seat" | "VehicleSeat")
}

pub fn is_gui_object(class: &str) -> bool {
    matches!(
        class,
        "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton" | "ScrollingFrame" | "ViewportFrame" | "CanvasGroup"
    )
}

/// Properties a freshly inserted instance gets (the dump carries no default values).
pub fn insert_defaults(class: &str, pos: [f32; 3]) -> Vec<(&'static str, Value)> {
    let mut v: Vec<(&'static str, Value)> = Vec::new();
    if is_base_part(class) {
        let size = if class == "SpawnLocation" { [6.0, 1.0, 6.0] } else { [4.0, 1.0, 2.0] };
        v.push(("Size", Value::Vector3(size)));
        v.push(("CFrame", cframe_at(pos)));
        v.push(("Color", Value::Color3([0.639, 0.635, 0.647])));
        v.push(("Material", Value::Enum(MATERIAL_PLASTIC)));
        v.push(("Anchored", Value::Bool(false)));
        if class == "Part" {
            v.push(("Shape", Value::Enum(SHAPE_BLOCK)));
        }
    }
    if is_gui_object(class) {
        v.push(("Size", Value::UDim2 { sx: 0.0, ox: 100, sy: 0.0, oy: 100 }));
        v.push(("Position", Value::UDim2 { sx: 0.0, ox: 0, sy: 0.0, oy: 0 }));
        v.push(("BackgroundColor3", Value::Color3([0.639, 0.635, 0.647])));
        v.push(("BorderSizePixel", Value::Int(0)));
        if matches!(class, "TextLabel" | "TextButton" | "TextBox") {
            v.push(("Text", Value::String(class.to_string())));
            v.push(("TextSize", Value::Int(14)));
            v.push(("TextColor3", Value::Color3([0.0, 0.0, 0.0])));
        }
    }
    match class {
        "Script" | "LocalScript" | "ModuleScript" => {
            let src = if class == "ModuleScript" { "local module = {}\n\nreturn module\n" } else { "print(\"Hello world!\")\n" };
            v.push(("Source", Value::String(src.to_string())));
        }
        "PointLight" | "SpotLight" | "SurfaceLight" => {
            v.push(("Brightness", Value::Float(1.0)));
            v.push(("Range", Value::Float(8.0)));
            v.push(("Color", Value::Color3([1.0, 1.0, 1.0])));
        }
        _ => {}
    }
    v
}

fn add(t: &mut Tree, class: &str, parent: Id, name: &str, props: Vec<(&str, Value)>) -> Id {
    let id = t.create(class);
    let _ = t.set_parent(id, Some(parent));
    let _ = t.set_prop(id, "Name", Value::String(name.to_string()));
    for (k, v) in props {
        let _ = t.set_prop(id, k, v);
    }
    id
}

/// Baseplate template: services, a baseplate, a spawn and Roblox-like lighting with a Sky.
pub fn baseplate() -> Tree {
    let mut t = Tree::new();
    let r = Tree::ROOT;
    let ws = add(&mut t, "Workspace", r, "Workspace", vec![("Gravity", Value::Float(196.2))]);
    add(
        &mut t,
        "Part",
        ws,
        "Baseplate",
        vec![
            ("Size", Value::Vector3([512.0, 20.0, 512.0])),
            ("CFrame", cframe_at([0.0, -10.0, 0.0])),
            ("Color", Value::Color3([0.388, 0.372, 0.384])),
            ("Material", Value::Enum(MATERIAL_PLASTIC)),
            ("Anchored", Value::Bool(true)),
            ("Shape", Value::Enum(SHAPE_BLOCK)),
        ],
    );
    add(
        &mut t,
        "SpawnLocation",
        ws,
        "SpawnLocation",
        vec![
            ("Size", Value::Vector3([6.0, 1.0, 6.0])),
            ("CFrame", cframe_at([0.0, 0.5, 0.0])),
            ("Color", Value::Color3([0.639, 0.635, 0.647])),
            ("Material", Value::Enum(MATERIAL_PLASTIC)),
            ("Anchored", Value::Bool(true)),
        ],
    );
    add(&mut t, "Camera", ws, "Camera", vec![]);
    let lighting = add(
        &mut t,
        "Lighting",
        r,
        "Lighting",
        vec![
            ("ClockTime", Value::Float(14.0)),
            ("Brightness", Value::Float(3.0)),
            ("Ambient", Value::Color3([0.0, 0.0, 0.0])),
            ("OutdoorAmbient", Value::Color3([0.5, 0.5, 0.5])),
            ("ColorShift_Top", Value::Color3([0.0, 0.0, 0.0])),
        ],
    );
    add(
        &mut t,
        "Sky",
        lighting,
        "Sky",
        vec![
            ("CelestialBodiesShown", Value::Bool(true)),
            ("StarCount", Value::Int(3000)),
            ("SunAngularSize", Value::Float(21.0)),
            ("MoonAngularSize", Value::Float(11.0)),
        ],
    );
    add(&mut t, "Players", r, "Players", vec![]);
    add(&mut t, "ReplicatedStorage", r, "ReplicatedStorage", vec![]);
    add(&mut t, "ServerScriptService", r, "ServerScriptService", vec![]);
    add(&mut t, "ServerStorage", r, "ServerStorage", vec![]);
    add(&mut t, "StarterGui", r, "StarterGui", vec![]);
    add(&mut t, "StarterPack", r, "StarterPack", vec![]);
    let sp = add(&mut t, "StarterPlayer", r, "StarterPlayer", vec![]);
    add(&mut t, "StarterPlayerScripts", sp, "StarterPlayerScripts", vec![]);
    add(&mut t, "StarterCharacterScripts", sp, "StarterCharacterScripts", vec![]);
    add(&mut t, "SoundService", r, "SoundService", vec![]);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseplate_has_expected_services() {
        let t = baseplate();
        for s in ["Workspace", "Lighting", "StarterGui", "ServerScriptService", "ReplicatedStorage"] {
            assert!(t.find_first_child(Tree::ROOT, s).is_some(), "{s}");
        }
        let ws = t.find_first_child(Tree::ROOT, "Workspace").unwrap();
        assert!(t.find_first_child(ws, "Baseplate").is_some());
        let l = t.find_first_child(Tree::ROOT, "Lighting").unwrap();
        assert!(t.find_first_child_of_class(l, "Sky").is_some());
    }

    #[test]
    fn defaults_per_class() {
        assert!(insert_defaults("Part", [0.0; 3]).iter().any(|(k, _)| *k == "Shape"));
        assert!(insert_defaults("TextButton", [0.0; 3]).iter().any(|(k, _)| *k == "Text"));
        assert!(insert_defaults("Script", [0.0; 3]).iter().any(|(k, _)| *k == "Source"));
        assert!(insert_defaults("Folder", [0.0; 3]).is_empty());
    }
}
