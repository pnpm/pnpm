use super::resolves_from_declaring_manifest;
use pnpm_resolving_resolver_base::{ResolveOptions, WantedDependency};
use std::{borrow::Cow, path::Path};

fn resolves_from_local_directory_manifest(spec: &str) -> bool {
    resolves_from_declaring_manifest(spec)
        || spec.starts_with("link:")
        || pnpm_local_spec::bare_path_is_unambiguous(spec)
}

pub(in crate::resolve_dependency_tree) fn opts_for_child_spec<'a>(
    opts: &'a ResolveOptions,
    wanted: &mut WantedDependency,
    parent_dir: Option<&Path>,
    fallback_dir: Option<&Path>,
) -> Cow<'a, ResolveOptions> {
    let spec = wanted.bare_specifier.as_deref().unwrap_or_default();
    if let Some(base_dir) = parent_dir.or(fallback_dir)
        && (spec.starts_with("link:") || pnpm_local_spec::bare_path_is_unambiguous(spec))
        && let Some(spec) = local_child_specifier(spec, base_dir)
    {
        wanted.bare_specifier = Some(spec);
        return Cow::Borrowed(opts);
    }
    let project_dir = match parent_dir {
        Some(parent_dir) if resolves_from_local_directory_manifest(spec) => Some(parent_dir),
        _ => None,
    };
    match project_dir {
        Some(project_dir) => Cow::Owned(ResolveOptions {
            project: pnpm_resolving_resolver_base::ResolverProjectOptions {
                project_dir: project_dir.to_path_buf(),
                ..opts.project.clone()
            },
            ..opts.clone()
        }),
        _ => Cow::Borrowed(opts),
    }
}

fn local_child_specifier(spec: &str, base_dir: &Path) -> Option<String> {
    pnpm_local_spec::LocalSpec::parse_filesystem(spec, base_dir).map(|spec| spec.render(None))
}

#[cfg(test)]
mod tests;
