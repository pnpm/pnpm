use derive_more::{Display, Error};
use pipe_trait::Pipe;
use serde::{Deserialize, Serialize};
use split_first_char::SplitFirstChar;
use std::{borrow::Cow, fmt, str::FromStr};

/// Represent the name of an npm package.
///
/// Syntax:
/// * Without scope: `{bare}`
/// * With scope: `@{scope}/bare`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Cow<'de, str>", into = "String")]
pub struct PkgName {
    /// The scope (if any) without the `@` prefix.
    pub scope: Option<String>,
    /// Either the whole package name (if without scope) or the bare name after the separator (if with scope).
    pub bare: String,
}

/// Error when parsing [`PkgName`] from a string input.
#[derive(Debug, Display, Error)]
pub enum ParsePkgNameError {
    #[display("Missing bare name")]
    MissingName,
    #[display("Name is empty")]
    EmptyName,
}

impl PkgName {
    /// Parse [`PkgName`] from a string input.
    pub fn parse<Input>(input: Input) -> Result<Self, ParsePkgNameError>
    where
        Input: Into<String> + AsRef<str>,
    {
        match input.as_ref().split_first_char() {
            Some(('@', rest)) => {
                let (scope, bare) = rest.split_once('/').ok_or(ParsePkgNameError::MissingName)?;
                let scope = scope.to_string().pipe(Some);
                let bare = bare.to_string();
                Ok(PkgName { scope, bare })
            }
            Some(_) => {
                let scope = None;
                let bare = input.into();
                Ok(PkgName { scope, bare })
            }
            None => Err(ParsePkgNameError::EmptyName),
        }
    }
}

impl TryFrom<String> for PkgName {
    type Error = ParsePkgNameError;
    fn try_from(input: String) -> Result<Self, Self::Error> {
        PkgName::parse(input)
    }
}

impl<'a> TryFrom<Cow<'a, str>> for PkgName {
    type Error = ParsePkgNameError;
    fn try_from(input: Cow<'a, str>) -> Result<Self, Self::Error> {
        PkgName::parse(input)
    }
}

impl FromStr for PkgName {
    type Err = ParsePkgNameError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        PkgName::parse(input)
    }
}

impl fmt::Display for PkgName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PkgName { scope, bare } = self;
        if let Some(scope) = scope {
            write!(f, "@{scope}/")?;
        }
        write!(f, "{bare}")
    }
}

impl From<PkgName> for String {
    fn from(value: PkgName) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests;
