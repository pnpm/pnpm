use derive_more::Error;
use miette::Diagnostic;
use pnpm_network::redact_and_sanitize;
use std::fmt;

use super::{ResolveDependencyTreeError, SkippedOptionalDependencyParent};

/// The failed edge and the packages that led to it. The source retains its
/// diagnostic code and help, including failures of locked optional edges.
#[derive(Debug, Error, Diagnostic)]
#[diagnostic(forward(source))]
pub struct DependencyResolutionError {
    pub alias: String,
    pub specifier: String,
    pub parents: Vec<SkippedOptionalDependencyParent>,
    pub prefix: String,
    #[error(source)]
    pub source: Box<ResolveDependencyTreeError>,
}

impl fmt::Display for DependencyResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\n\nFailed to resolve {}@{}",
            self.source,
            redact_and_sanitize(&self.alias),
            redact_and_sanitize(&self.specifier),
        )?;
        for parent in &self.parents {
            write!(
                formatter,
                "\nThis error happened while installing the dependencies of {}@{}",
                redact_and_sanitize(&parent.name),
                redact_and_sanitize(&parent.version),
            )?;
        }
        Ok(())
    }
}
