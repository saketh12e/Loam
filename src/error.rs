use std::fmt;

/// Engine error type. Variants carry just enough context to be useful in
/// diagnostics without exposing internal buffer state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Input ended before a structure was complete.
    UnexpectedEof { what: &'static str },
    /// A magic or version field did not match.
    BadMagic { what: &'static str },
    /// A declared count or length exceeded its container.
    LengthMismatch { what: &'static str },
    /// A checksum did not verify.
    Checksum { what: &'static str },
    /// An encoding tag or type tag is not recognized for this file version.
    UnsupportedTag { tag: u32 },
    /// A schema column reference did not resolve.
    UnknownColumn { id: u32 },
    /// The expression text did not parse or compile.
    Expression { detail: String },
    /// A plan bytecode stream was malformed.
    Plan { detail: String },
    /// An operator received values of an incompatible type.
    TypeMismatch { what: &'static str },
    /// A session script operation was malformed or out of order.
    Session { detail: String },
    /// A spill segment was malformed or referred to absent state.
    Spill { detail: String },
    /// A view reference did not resolve.
    View { detail: String },
    /// An internal budget was exceeded (memory, rows, pages).
    BudgetExceeded { what: &'static str },
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnexpectedEof { what } => write!(f, "unexpected end of input reading {what}"),
            Error::BadMagic { what } => write!(f, "bad magic or version for {what}"),
            Error::LengthMismatch { what } => {
                write!(f, "declared length exceeds container for {what}")
            }
            Error::Checksum { what } => write!(f, "checksum mismatch for {what}"),
            Error::UnsupportedTag { tag } => write!(f, "unsupported tag {tag}"),
            Error::UnknownColumn { id } => write!(f, "unknown column {id}"),
            Error::Expression { detail } => write!(f, "expression error: {detail}"),
            Error::Plan { detail } => write!(f, "plan error: {detail}"),
            Error::TypeMismatch { what } => write!(f, "type mismatch in {what}"),
            Error::Session { detail } => write!(f, "session error: {detail}"),
            Error::Spill { detail } => write!(f, "spill error: {detail}"),
            Error::View { detail } => write!(f, "view error: {detail}"),
            Error::BudgetExceeded { what } => write!(f, "budget exceeded for {what}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_covers_all_variants() {
        let errs = vec![
            Error::UnexpectedEof { what: "header" },
            Error::BadMagic { what: "file" },
            Error::LengthMismatch { what: "page" },
            Error::Checksum { what: "footer" },
            Error::UnsupportedTag { tag: 9 },
            Error::UnknownColumn { id: 3 },
            Error::Expression { detail: "x".into() },
            Error::Plan { detail: "x".into() },
            Error::TypeMismatch { what: "filter" },
            Error::Session { detail: "x".into() },
            Error::Spill { detail: "x".into() },
            Error::View { detail: "x".into() },
            Error::BudgetExceeded { what: "arena" },
        ];
        for e in errs {
            assert!(!format!("{e}").is_empty());
        }
    }
}
