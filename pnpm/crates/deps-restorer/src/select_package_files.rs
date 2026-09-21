//! Which files of a package an install imports, under
//! [`pnpm_config::Config::package_import_patterns`].

use pnpm_matcher::create_matcher;
use std::{borrow::Cow, collections::HashMap, path::PathBuf};

/// The part of a package's file index to import: all of it when `patterns` is empty, and
/// otherwise the files whose path matches, with the `package.json` at the root of the package
/// whatever the patterns say. A package directory without that file reads as an interrupted
/// import, which every later install would repeat.
#[must_use]
pub fn select_package_files<'files>(
    cas_paths: &'files HashMap<String, PathBuf>,
    patterns: &[String],
) -> Cow<'files, HashMap<String, PathBuf>> {
    if patterns.is_empty() {
        return Cow::Borrowed(cas_paths);
    }
    let matcher = create_matcher(patterns);
    Cow::Owned(
        cas_paths
            .iter()
            .filter(|(path, _)| path.as_str() == "package.json" || matcher.matches(path))
            .map(|(path, cas_path)| (path.clone(), cas_path.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
