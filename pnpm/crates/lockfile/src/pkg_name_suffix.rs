use crate::{
    ParsePkgNameError,
    PkgName,
};
use derive_more::{
    Display,
    Error,
};
use pnpm_pkg_name_split::{
    SplitNameSuffixError,
    split_name_suffix,
};
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    borrow::Cow,
    str::FromStr,
};

/// Syntax: `{name}@{suffix}`
///
/// Examples:
/// * `ts-node@10.9.1`, `@types/node@18.7.19`, `typescript@5.1.6`
/// * `react-json-view@1.21.3(@types/react@17.0.49)(react-dom@17.0.2)(react@17.0.2)`
#[derive(Debug, Display, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[display(bound(Suffix: std::fmt::Display))]
#[display("{name}@{suffix}")]
#[serde(try_from = "Cow<'de, str>", into = "String")]
#[serde(bound(
    deserialize = "Suffix: FromStr, Suffix::Err: std::fmt::Display",
    serialize = "Suffix: std::fmt::Display + Clone",
))]
pub struct PkgNameSuffix<Suffix> {
    pub name: PkgName,
    pub suffix: Suffix,
}

impl<Suffix> PkgNameSuffix<Suffix> {
    /// Construct a [`PkgNameSuffix`].
    pub fn new(name: PkgName, suffix: Suffix) -> Self {
        PkgNameSuffix { name, suffix }
    }
}

/// Error when parsing [`PkgNameSuffix`] from a string.
#[derive(Debug, Display, Error)]
#[display(bound(ParseSuffixError: Display))]
pub enum ParsePkgNameSuffixError<ParseSuffixError> {
    #[display("Input is empty")]
    EmptyInput,
    #[display("Suffix is missing")]
    MissingSuffix,
    #[display("Name is empty")]
    EmptyName,
    #[display("Failed to parse suffix: {_0}")]
    ParseSuffixFailure(#[error(source)] ParseSuffixError),
    #[display("Failed to parse name: {_0}")]
    ParseNameFailure(#[error(source)] ParsePkgNameError),
}

impl<Suffix: FromStr> FromStr for PkgNameSuffix<Suffix> {
    type Err = ParsePkgNameSuffixError<Suffix::Err>;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, suffix) = split_name_suffix(value)
            .map_err(|error| match error {
                SplitNameSuffixError::EmptyInput => ParsePkgNameSuffixError::EmptyInput,
                SplitNameSuffixError::MissingSuffix => ParsePkgNameSuffixError::MissingSuffix,
            })?;
        // `split_name_suffix` never returns an empty name; Creusot proves it.
        if matches!(name, "@" | "@/") {
            return Err(ParsePkgNameSuffixError::EmptyName);
        }
        if suffix.is_empty() {
            return Err(ParsePkgNameSuffixError::MissingSuffix);
        }
        let suffix =
            suffix.parse::<Suffix>().map_err(ParsePkgNameSuffixError::ParseSuffixFailure)?;
        let name = name.parse().map_err(ParsePkgNameSuffixError::ParseNameFailure)?;
        Ok(PkgNameSuffix { name, suffix })
    }
}

impl<'a, Suffix: FromStr> TryFrom<Cow<'a, str>> for PkgNameSuffix<Suffix> {
    type Error = ParsePkgNameSuffixError<Suffix::Err>;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl<Suffix: std::fmt::Display> From<PkgNameSuffix<Suffix>> for String {
    fn from(value: PkgNameSuffix<Suffix>) -> Self {
        value.to_string()
    }
}
