//! The parts of a [SCIP](https://github.com/sourcegraph/scip) index that
//! [`crate::program`] reads. The tags are those of `scip.proto`.

use prost::{
    Message,
    bytes::Bytes,
};
use std::{
    fs,
    path::Path,
};

/// The `SymbolRole.Definition` bit of [`Occurrence::symbol_roles`].
pub const DEFINITION: i32 = 1;

/// The `SymbolInformation.Kind` of a struct, union, or enum variant field.
pub const FIELD: i32 = 15;

/// The `SymbolInformation.Kind` values of a function: `Function`, `Method`,
/// `TraitMethod`, and `StaticMethod`.
pub const FUNCTIONS: [i32; 4] = [17, 26, 70, 80];

#[derive(Message)]
pub struct Index {
    /// Each `Document`, still encoded, so that one is decoded at a time.
    #[prost(bytes = "bytes", repeated, tag = "2")]
    pub documents: Vec<Bytes>,
}

/// The symbols a `Document` defines.
#[derive(Message)]
pub struct Definitions {
    #[prost(string, tag = "1")]
    pub relative_path: String,
    #[prost(message, repeated, tag = "3")]
    pub symbols: Vec<SymbolInformation>,
}

#[derive(Message)]
pub struct SymbolInformation {
    #[prost(string, tag = "1")]
    pub symbol: String,
    #[prost(int32, tag = "5")]
    pub kind: i32,
    #[prost(string, tag = "6")]
    pub display_name: String,
}

/// The places where a `Document` names a symbol.
#[derive(Message)]
pub struct Occurrences {
    #[prost(string, tag = "1")]
    pub relative_path: String,
    #[prost(message, repeated, tag = "2")]
    pub occurrences: Vec<Occurrence>,
}

#[derive(Message)]
pub struct Occurrence {
    /// `[line, start, end]` or `[start line, start, end line, end]`, counted
    /// from zero, with columns in UTF-8 bytes.
    #[prost(int32, repeated, tag = "1")]
    pub range: Vec<i32>,
    #[prost(string, tag = "2")]
    pub symbol: String,
    #[prost(int32, tag = "3")]
    pub symbol_roles: i32,
    /// On a definition, the range of the whole item.
    #[prost(int32, repeated, tag = "7")]
    pub enclosing_range: Vec<i32>,
}

pub fn read(path: &Path) -> Result<Index, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {path:?}: {error}"))?;
    Index::decode(Bytes::from(bytes))
        .map_err(|error| format!("{path:?} is not a SCIP index: {error}"))
}

/// Decodes one of [`Index::documents`] as `View`, which is [`Definitions`]
/// or [`Occurrences`].
pub fn decode<View: Message + Default>(document: &Bytes) -> Result<View, String> {
    View::decode(document.clone())
        .map_err(|error| format!("the SCIP index holds a malformed document: {error}"))
}
