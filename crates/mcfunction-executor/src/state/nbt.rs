use std::fmt;

/// A mutable NBT value for the in-process storage engine.
///
/// This is separate from `mdl_compiler::ir::minecraft::NbtValue` because
/// the compiler's type is immutable (the `kind` field is `pub(super)`).
/// We need in-place mutation for `data modify` operations.
#[derive(Clone, Debug, PartialEq)]
pub enum NbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    List(Vec<NbtValue>),
    Compound(Vec<(String, NbtValue)>),
}

impl NbtValue {
    /// Returns a borrowed view of this value.
    pub fn as_ref(&self) -> NbtValueRef<'_> {
        match self {
            Self::Byte(v) => NbtValueRef::Byte(*v),
            Self::Short(v) => NbtValueRef::Short(*v),
            Self::Int(v) => NbtValueRef::Int(*v),
            Self::Long(v) => NbtValueRef::Long(*v),
            Self::Float(v) => NbtValueRef::Float(*v),
            Self::Double(v) => NbtValueRef::Double(*v),
            Self::String(v) => NbtValueRef::String(v),
            Self::List(v) => NbtValueRef::List(v),
            Self::Compound(v) => NbtValueRef::Compound(v),
        }
    }

    pub fn is_compound(&self) -> bool {
        matches!(self, Self::Compound(_))
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Self::List(_))
    }

    #[must_use]
    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Byte(v) => i32::from(*v),
            Self::Short(v) => i32::from(*v),
            Self::Int(v) => *v,
            Self::Long(v) => *v as i32,
            Self::Float(v) => *v as i32,
            Self::Double(v) => *v as i32,
            Self::String(v) => v.len() as i32,
            Self::List(v) => v.len() as i32,
            Self::Compound(_) => 1,  // data get on compound root returns 1
        }
    }

    pub fn compound(entries: Vec<(String, NbtValue)>) -> Self {
        let mut sorted = entries;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        Self::Compound(sorted)
    }

    /// Converts to the compiler's NBT type for display/log output.
    pub fn to_snbt(&self) -> String {
        match self {
            Self::Byte(v) => format!("{v}b"),
            Self::Short(v) => format!("{v}s"),
            Self::Int(v) => format!("{v}"),
            Self::Long(v) => format!("{v}L"),
            Self::Float(v) => {
                if *v == -0.0 { "0.0f".to_owned() } else { format!("{v}f") }
            }
            Self::Double(v) => {
                if *v == -0.0 { "0.0d".to_owned() } else { format!("{v}d") }
            }
            Self::String(v) => {
                let escaped: String = v.chars().map(|c| match c {
                    '"' => "\\\"".to_owned(),
                    '\\' => "\\\\".to_owned(),
                    '\n' => "\\n".to_owned(),
                    '\r' => "\\r".to_owned(),
                    '\t' => "\\t".to_owned(),
                    c => c.to_string(),
                }).collect();
                format!("\"{escaped}\"")
            }
            Self::List(values) => {
                let inner: Vec<String> = values.iter().map(Self::to_snbt).collect();
                format!("[{}]", inner.join(","))
            }
            Self::Compound(entries) => {
                let inner: Vec<String> = entries.iter().map(|(k, v)| {
                    format!("{k}:{}", v.to_snbt())
                }).collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }
}

/// Borrowed view of an NBT value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NbtValueRef<'a> {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(&'a str),
    List(&'a [NbtValue]),
    Compound(&'a [(String, NbtValue)]),
}

impl fmt::Display for NbtValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_snbt())
    }
}
