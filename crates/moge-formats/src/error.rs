use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum FormatError {
    BadMagic,
    Truncated(&'static str),
    Unsupported(String),
    Malformed(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::BadMagic => write!(f, "Unrecognized file format: not a Roblox place or model file"),
            FormatError::Truncated(what) => write!(f, "Unexpected end of file while reading {what}"),
            FormatError::Unsupported(m) => write!(f, "Unsupported file feature: {m}"),
            FormatError::Malformed(m) => write!(f, "Malformed file: {m}"),
        }
    }
}

impl std::error::Error for FormatError {}
