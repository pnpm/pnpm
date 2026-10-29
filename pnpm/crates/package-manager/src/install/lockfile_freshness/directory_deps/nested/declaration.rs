//! Whether a registry package's nested directory dependency was declared
//! with `file:` or with `workspace:`, read from the settings that add one.

use super::{ImporterSatisfactionCheck, NestedDirectoryPackage};
use crate::PackageExtender;
use pnpm_lockfile::PackageKey;
use std::path::Path;

/// How a spec declares a local directory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Declaration {
    File,
    Workspace,
}

/// How the settings that apply to every edge into `nested` declare it.
/// `None` when some edge takes it from no setting, such as one a
/// `readPackage` hook adds, or when the edges disagree.
pub(super) fn settings_declaration(
    check: &ImporterSatisfactionCheck<'_>,
    nested: &NestedDirectoryPackage,
) -> Option<Declaration> {
    let extender = check.config.package_extensions
        .as_ref()
        .and_then(|extensions| PackageExtender::new(extensions).ok());
    let edges = EdgeSettings {
        check,
        extender: extender.as_ref(),
        target: &pnpm_fs::lexical_normalize(&check.lockfile_dir.join(&nested.directory)),
    };
    agreed(
        nested.parents
            .iter()
            .map(|(parent, alias)| edges.declaration(parent, alias)),
    )
}

/// The settings that can give an edge its spec, and the directory checked.
struct EdgeSettings<'a> {
    check: &'a ImporterSatisfactionCheck<'a>,
    extender: Option<&'a PackageExtender>,
    target: &'a Path,
}

impl EdgeSettings<'_> {
    /// How `parent`'s dependency `alias` declares the directory. An
    /// override applies after the package extensions, so it decides when
    /// one matches.
    fn declaration(&self, parent: &PackageKey, alias: &str) -> Option<Declaration> {
        let parent_name = parent.name.to_string();
        let overrides = self.check.parsed_overrides
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.target_pkg.name == alias
                    && entry.parent_pkg
                        .as_ref()
                        .is_none_or(|selector| selector.name == parent_name)
            })
            .map(|entry| self.declares_target(&entry.new_bare_specifier));
        agreed(overrides)
            .or_else(|| {
                let spec = self.extended_spec(parent, alias)?;
                self.declares_target(&spec)
            })
    }

    /// The spec the package extensions give `parent`'s dependency `alias`.
    fn extended_spec(&self, parent: &PackageKey, alias: &str) -> Option<String> {
        let mut manifest = serde_json::json!({
            "name": parent.name.to_string(),
            "version": parent.suffix.version().to_string(),
        });
        self.extender?.apply(&mut manifest);
        ["dependencies", "optionalDependencies"]
            .into_iter()
            .find_map(|field| {
                manifest
                    .get(field)?
                    .get(alias)?
                    .as_str()
                    .map(str::to_string)
            })
    }

    /// How `spec` declares the checked directory, `None` when it names
    /// another one or none.
    fn declares_target(&self, spec: &str) -> Option<Declaration> {
        let (path, declaration) = declared_directory(spec)?;
        (pnpm_fs::lexical_normalize(&self.check.lockfile_dir.join(path)) == self.target).then_some(
            declaration,
        )
    }
}

/// The one declaration every item gives, `None` when one gives none or two
/// disagree.
fn agreed(mut declarations: impl Iterator<Item = Option<Declaration>>) -> Option<Declaration> {
    let first = declarations.next()??;
    declarations
        .all(|declaration| declaration == Some(first))
        .then_some(first)
}

/// The directory a spec names and how it declares it.
fn declared_directory(spec: &str) -> Option<(&str, Declaration)> {
    if let Some(path) = spec.strip_prefix("workspace:") {
        return pnpm_local_spec::is_filespec(path).then_some((path, Declaration::Workspace));
    }
    let path = spec.strip_prefix("file:").unwrap_or(spec);
    let names_directory = (path.len() < spec.len() || pnpm_local_spec::is_filespec(path))
        && !pnpm_lockfile::is_local_tarball_path(path);
    names_directory.then_some((path, Declaration::File))
}
