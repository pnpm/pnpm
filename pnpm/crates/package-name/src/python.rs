use derive_more::{Display, Error};
use pep508_rs::PackageName as PythonPackageName;
use std::str::FromStr;

#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("{name:?} is not a valid Python project name")]
pub struct PythonNameError {
    pub name: String,
}

/// The PEP 503 normalized form of a Python project name.
pub fn canonicalize_python_name(raw: &str) -> Result<String, PythonNameError> {
    let invalid = || PythonNameError { name: raw.to_string() };
    let normalized = PythonPackageName::from_str(raw)
        .map_err(|_| invalid())?
        .as_ref()
        .to_string();
    let well_formed = normalized
        .chars()
        .all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        && normalized
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && normalized
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_alphanumeric());
    well_formed.then_some(normalized).ok_or_else(invalid)
}
