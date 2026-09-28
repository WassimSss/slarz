//! The values a running script works with.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Integer(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    /// What a function without a return type gives back.
    Nothing,
}

impl Value {
    /// The Slarz name of the value's type, as written in scripts.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "Int",
            Self::Float(_) => "Float",
            Self::Text(_) => "Text",
            Self::Bool(_) => "Bool",
            Self::Nothing => "Nothing",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(f, "{value}"),
            // Debug formatting keeps the decimal point: `3.0`, not `3`.
            Self::Float(value) => write!(f, "{value:?}"),
            Self::Text(text) => write!(f, "{text}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Nothing => write!(f, "nothing"),
        }
    }
}
