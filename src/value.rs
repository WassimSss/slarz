//! The values a running script works with.

use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Integer(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    /// Shared, not copied: cloning a list only bumps a counter. The list is
    /// really copied only when someone modifies it while it is shared
    /// (`Rc::make_mut`), so `b = a` stays cheap and `a` never changes behind
    /// `b`'s back.
    List(Rc<Vec<Value>>),
    /// What a function without a return type gives back.
    Nothing,
    /// The two sides of a `Result`: an operation that worked, or why it failed.
    Success(Box<Value>),
    Failure(String),
}

impl Value {
    /// The Slarz name of the value's type, as written in scripts.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Integer(_) => "Int",
            Self::Float(_) => "Float",
            Self::Text(_) => "Text",
            Self::Bool(_) => "Bool",
            Self::List(_) => "List",
            Self::Nothing => "Nothing",
            Self::Success(_) | Self::Failure(_) => "Result",
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
            Self::List(items) => {
                write!(f, "[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    // Quoted inside a list, so `["a, b"]` and `["a", "b"]` differ.
                    match item {
                        Self::Text(text) => write!(f, "{text:?}")?,
                        other => write!(f, "{other}")?,
                    }
                }
                write!(f, "]")
            }
            Self::Nothing => write!(f, "nothing"),
            Self::Success(value) => write!(f, "Ok({value})"),
            Self::Failure(message) => write!(f, "Error({message})"),
        }
    }
}
