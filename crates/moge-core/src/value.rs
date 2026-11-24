pub type Id = usize;

/// Property value. Strings cover String/Content/ProtectedString; `Raw` preserves unknown types.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Vector2([f32; 2]),
    Vector3([f32; 3]),
    Color3([f32; 3]),
    UDim(f32, i32),
    UDim2 { sx: f32, ox: i32, sy: f32, oy: i32 },
    /// `rot` is row-major: R00 R01 R02 R10 ... R22.
    CFrame { pos: [f32; 3], rot: [f32; 9] },
    Rect { min: [f32; 2], max: [f32; 2] },
    NumberRange(f32, f32),
    NumberSequence(Vec<[f32; 3]>),
    ColorSequence(Vec<(f32, [f32; 3])>),
    BrickColor(u32),
    Enum(u32),
    Ref(Option<Id>),
    Font { family: String, weight: u16, style: u8 },
    Faces(u8),
    Axes(u8),
    Raw { ty: String, text: String },
}

pub const IDENTITY_ROT: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Bytes(_) => "BinaryString",
            Value::Vector2(_) => "Vector2",
            Value::Vector3(_) => "Vector3",
            Value::Color3(_) => "Color3",
            Value::UDim(..) => "UDim",
            Value::UDim2 { .. } => "UDim2",
            Value::CFrame { .. } => "CFrame",
            Value::Rect { .. } => "Rect",
            Value::NumberRange(..) => "NumberRange",
            Value::NumberSequence(_) => "NumberSequence",
            Value::ColorSequence(_) => "ColorSequence",
            Value::BrickColor(_) => "BrickColor",
            Value::Enum(_) => "Enum",
            Value::Ref(_) => "Instance",
            Value::Font { .. } => "Font",
            Value::Faces(_) => "Faces",
            Value::Axes(_) => "Axes",
            Value::Raw { .. } => "Raw",
        }
    }

    /// Zero value for a value-type name from the API Dump (None when unknown).
    pub fn default_for(type_name: &str) -> Option<Value> {
        Some(match type_name {
            "bool" => Value::Bool(false),
            "int" | "int64" => Value::Int(0),
            "float" | "double" => Value::Float(0.0),
            "string" | "Content" | "ProtectedString" | "BinaryString" | "SharedString" => Value::String(String::new()),
            "Vector2" => Value::Vector2([0.0; 2]),
            "Vector3" => Value::Vector3([0.0; 3]),
            "Color3" => Value::Color3([0.0; 3]),
            "UDim" => Value::UDim(0.0, 0),
            "UDim2" => Value::UDim2 { sx: 0.0, ox: 0, sy: 0.0, oy: 0 },
            "CFrame" => Value::CFrame { pos: [0.0; 3], rot: IDENTITY_ROT },
            "BrickColor" => Value::BrickColor(194),
            "Rect" => Value::Rect { min: [0.0; 2], max: [0.0; 2] },
            "NumberRange" => Value::NumberRange(0.0, 0.0),
            _ => return None,
        })
    }
}
