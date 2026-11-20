use super::resolves_from_declaring_manifest;
use pnpm_resolving_resolver_base::{
    ResolveOptions,
    WantedDependency,
};
use std::{
    borrow::Cow,
    path::Path,
};

/// Point a child's local specifier at the directory it is written from.
///
/// - A `link:` or bare-path child is written from `parent_dir`, or from
///   `fallback_dir` when its parent has no directory. Its specifier is
///   rewritten to name the same target from the importer, because the
///   `link:` id the resolver returns is read relative to the importer.
/// - A specifier matching [`resolves_from_declaring_manifest`] resolves
///   with `parent_dir` as the project directory.
/// - Every other specifier keeps the caller's options.
pub(in crate::resolve_dependency_tree) fn opts_for_child_spec<'a>(
    opts: &'a ResolveOptions,
    wanted: &mut WantedDependency,
    parent_dir: Option<&Path>,
    fallback_dir: Option<&Path>,
) -> Cow<'a, ResolveOptions> {
    let spec = wanted.bare_specifier.as_deref().unwrap_or_default();
    if let Some(base_dir) = parent_dir.or(fallback_dir)
        && let Some(spec) = local_child_specifier(spec, base_dir, &opts.project.project_dir)
    {
        wanted.bare_specifier = Some(spec);
        return Cow::Borrowed(opts);
    }
    match parent_dir {
        Some(parent_dir) if resolves_from_declaring_manifest(spec) => Cow::Owned(ResolveOptions {
            project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                project_dir: parent_dir.to_path_buf(),
                ..opts.project.clone()
            },
            ..opts.clone()
        }),
        _ => Cow::Borrowed(opts),
    }
}

/// The `link:` or bare-path `spec` written from `base_dir`, as the importer
/// names it. `None` for every other specifier.
///
/// A bare path stays relative: an absolute one opens with a drive letter on
/// Windows, which a named-registry alias claims first.
fn local_child_specifier(spec: &str, base_dir: &Path, importer_dir: &Path) -> Option<String> {
    let is_link = spec.starts_with("link:");
    if !is_link && !pnpm_local_spec::bare_path_is_unambiguous(spec) {
        return None;
    }
    pnpm_local_spec::LocalSpec::parse_filesystem(spec, base_dir)
        .map(|local_spec| local_spec.render((!is_link).then_some(importer_dir)))
}

#[cfg(test)]
mod tests;
