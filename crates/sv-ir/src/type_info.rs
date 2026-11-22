//! The declared type of a name, as an expression typer needs it.

/// A type as an expression typer needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    /// `Type::toString()`.
    pub text: String,
    /// An integral type (packed): its width, signedness and four states.
    pub integral: Option<(u32, bool, bool)>,
    /// Whether the type holds 4-state (`x`/`z`) values (including unpacked arrays/structs).
    pub four_state: bool,
    /// The data type it was declared with, to look into (struct members).
    pub src: Option<(usize, sv_syntax::NodeId)>,
    /// An enum, struct or union (also through typedefs): binary operators
    /// do not keep it as their type.
    pub aggregate: bool,
}

impl TypeInfo {
    /// A type with only its printed form: not integral, not an aggregate, no
    /// declaration to look into.
    pub fn other(text: impl Into<String>) -> TypeInfo {
        TypeInfo { text: text.into(), integral: None, four_state: false, src: None, aggregate: false }
    }
}
