use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, num::ParseIntError, str::FromStr};

/// Information of the top-level field `lockfileVersion`.
///
/// It contains only major and minor.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[display("{major}.{minor}")]
#[serde(try_from = "Cow<'de, str>", into = "String")]
pub struct ComVer {
    pub major: u16,
    pub minor: u16,
}

impl ComVer {
    /// Create a comver struct.
    #[must_use]
    pub fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// Error when parsing [`ComVer`] from a string.
#[derive(Debug, Display, Error)]
pub enum ParseComVerError {
    #[display("Dot is missing")]
    MissingDot,
    #[display("Major is not a valid number: {_0}")]
    InvalidMajor(ParseIntError),
    #[display("Minor is not a valid number: {_0}")]
    InvalidMinor(ParseIntError),
}

impl FromStr for ComVer {
    type Err = ParseComVerError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (major, minor) = text.split_once('.').ok_or(ParseComVerError::MissingDot)?;
        let major = major.parse::<u16>().map_err(ParseComVerError::InvalidMajor)?;
        let minor = minor.parse::<u16>().map_err(ParseComVerError::InvalidMinor)?;
        Ok(ComVer::new(major, minor))
    }
}

impl<'a> TryFrom<Cow<'a, str>> for ComVer {
    type Error = ParseComVerError;
    fn try_from(value: Cow<'a, str>) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ComVer> for String {
    fn from(value: ComVer) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests;
