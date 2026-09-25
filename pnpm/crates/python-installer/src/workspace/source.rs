//! How one `[tool.uv.sources]` entry is read: which source it names, and
//! where that leaves the project it points at.

use super::{
    super::manifest::{Manifest, Source, SourceDeclaration},
    LocalProject, parse_requirement,
};
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::{PackageName, Requirement, VerbatimUrl};
use std::path::{Path, PathBuf};

/// A source declaration, and the project directory that declared it. A
/// relative path means the same thing to every project that inherits the
/// declaration: the directory the declaring manifest sits in.
pub(super) struct Declared<'a> {
    pub(super) declaration: &'a SourceDeclaration,
    pub(super) by: &'a Path,
}

/// The one source a declaration names. A requirement is resolved from one
/// place, so a declaration offering several is refused.
pub(super) fn sole_source<'a>(
    declaration: &'a SourceDeclaration,
    name: &PackageName,
    manifest_path: &Path,
) -> Result<&'a Source> {
    match declaration {
        SourceDeclaration::One(source) => Ok(source),
        SourceDeclaration::Many(sources) => {
            let manifest = manifest_path.display();
            let count = sources.len();
            bail!(
                "pnpm resolves a Python requirement from one source, but {manifest} declares \
                 {count} of them for `{name}`",
            )
        }
    }
}

/// Refuse a source pnpm cannot resolve, rather than falling back to the
/// index and installing something else under the name.
pub(super) fn reject_unresolvable(
    source: &Source,
    name: &PackageName,
    manifest_path: &Path,
) -> Result<()> {
    let kind = if source.index.is_some() {
        "index"
    } else if let Some(narrowed) = source.narrowing.kind() {
        narrowed
    } else if source.path.is_none()
        && !source.workspace
        && source.git.is_none()
        && source.url.is_none()
    {
        "empty"
    } else {
        return Ok(());
    };
    let manifest = manifest_path.display();
    bail!("pnpm does not support the {kind} Python source {manifest} declares for `{name}`")
}

/// Where a path source points, refusing a path whose `..` the platform
/// and this reading would follow to different directories.
///
/// A `..` after a symlink goes to the link target's parent on POSIX and
/// to the directory the path was written in on Windows, and a `..` after
/// a directory that is not there is an error on POSIX and nothing on
/// Windows. pnpm reads the path itself, so that what the lockfile records
/// means one thing everywhere, and refuses the shapes where reading it
/// that way would differ from following it.
pub(super) fn path_target(
    declared_by: &Path,
    path: &str,
    name: &PackageName,
    manifest_path: &Path,
) -> Result<PathBuf> {
    let mut walked = declared_by.to_path_buf();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let found = std::fs::symlink_metadata(&walked);
                match found {
                    Ok(metadata) if metadata.is_symlink() => bail!(
                        "{} declares `{name}` at {path}, whose `..` follows the link {}. Write \
                         the path the link leads to, which means the same directory everywhere.",
                        manifest_path.display(),
                        walked.display(),
                    ),
                    Ok(_) => {}
                    Err(_) => bail!(
                        "{} declares `{name}` at {path}, whose `..` follows {}, which is not \
                         there",
                        manifest_path.display(),
                        walked.display(),
                    ),
                }
                walked.pop();
            }
            component => walked.push(component),
        }
    }
    if !walked.is_dir() {
        bail!(
            "{} declares `{name}` at {}, which is not a directory",
            manifest_path.display(),
            declared_by.join(path).display(),
        );
    }
    Ok(walked)
}

impl super::Workspace {
    /// The source a project resolves a distribution from: its own
    /// declaration, else the one its workspace root declares. A member
    /// inherits the root's table, so a workspace can say once where each
    /// of its projects comes from.
    pub(super) fn source<'a>(
        &'a self,
        root: &'a Path,
        manifest: &'a Manifest,
        name: &PackageName,
    ) -> Option<Declared<'a>> {
        if let Some(declaration) = manifest.tool.uv.sources.get(name) {
            return Some(Declared { declaration, by: root });
        }
        let (declaring_root, inherited) = self.inherited.get(root)?;
        Some(Declared { declaration: inherited.tool.uv.sources.get(name)?, by: declaring_root })
    }

    pub(crate) fn requirements(
        &self,
        root: &Path,
        manifest: &Manifest,
        requirements: Vec<Requirement>,
    ) -> Result<Vec<Requirement>> {
        let mut explicit = Vec::new();
        for requirement in &requirements {
            let Some(Declared { declaration, by }) = self.source(root, manifest, &requirement.name)
            else {
                continue;
            };
            let source = sole_source(declaration, &requirement.name, &by.join("pyproject.toml"))?;
            reject_unresolvable(source, &requirement.name, &by.join("pyproject.toml"))?;
            if let Some(url) = super::super::sources::declaration_url(source)? {
                let mut direct = requirement.clone();
                direct.version_or_url = Some(pep508_rs::VersionOrUrl::Url(
                    VerbatimUrl::parse_url(&url).into_diagnostic()?,
                ));
                explicit.push(direct);
            }
        }
        explicit.extend(requirements);
        Ok(explicit)
    }

    pub(super) fn project_requirements(
        &self,
        project: &mut LocalProject,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<()> {
        let requirements = project.metadata.requires_dist
            .iter()
            .map(|requirement| parse_requirement(requirement))
            .collect::<Result<Vec<_>>>()?;
        project.metadata.requires_dist = self
            .requirements(root, manifest, requirements)?
            .into_iter()
            .map(|requirement| requirement.to_string())
            .collect();
        Ok(())
    }
}
