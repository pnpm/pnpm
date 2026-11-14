use super::SkillSource;
use crate::allow_build_key_from_ignored_build;
use pnpm_deps_restorer::{is_local_directory_version, parse_dep_path_name_version};
use std::collections::HashMap;

/// Whether `dep_path` names a registry version (`<name>@<semver>`).
pub(super) fn is_registry_dep_path(dep_path: &str) -> bool {
    registry_version(dep_path).is_some()
}

/// The parsed semver of a registry `dep_path`, or `None` for non-registry
/// sources.
pub(super) fn registry_version(dep_path: &str) -> Option<node_semver::Version> {
    let (_, version) = parse_dep_path_name_version(dep_path)?;
    node_semver::Version::parse(version).ok()
}

/// Whether `dep_path` is a `link:` local directory dependency.
pub(super) fn is_link_dep_path(dep_path: &str) -> bool {
    parse_dep_path_name_version(dep_path).is_some_and(|(_, version)| version.starts_with("link:"))
}

/// The `(package_name, normalized_rel_dir)` of a `link:` or non-tarball `file:`
/// local directory `dep_path`.
pub(super) fn local_dir_identity(dep_path: &str) -> Option<(&str, String)> {
    let (name, version) = parse_dep_path_name_version(dep_path)?;
    if !is_local_directory_version(version) {
        return None;
    }
    let raw = version
        .strip_prefix("link:")
        .or_else(|| version.strip_prefix("file:"))?;
    let normalized = pnpm_fs::lexical_normalize_posix(&raw.replace('\\', "/"));
    let trimmed = normalized.trim_end_matches('/');
    let dir = if trimmed.is_empty() { "/" } else { trimmed };
    Some((name, dir.to_string()))
}

/// The deduplication key for [`super::keep_highest`]: bare package name for
/// registry versions, `<name>@dir:<normalized_dir>` for local directories (so
/// `workspace:*` `link:` and injected `file:` copies of the same directory
/// collapse to one source), and the peer-free `allowBuilds` key for git and
/// tarball sources.
pub(super) fn source_identity_key(dep_path: &str) -> String {
    if let Some((name, dir)) = local_dir_identity(dep_path) {
        return format!("{name}@dir:{dir}");
    }
    allow_build_key_from_ignored_build(dep_path)
}

/// Ensure local directory sources whose project alias collides with another
/// discovered source receive a distinct, deterministic `link_segment`, while
/// keeping single-source and registry/git segments unchanged.
pub(super) fn disambiguate_local_segments(sources: &mut [SkillSource]) {
    let base_segments: Vec<String> = sources
        .iter()
        .map(|source| source.link_segment.clone())
        .collect();
    let counts = segment_counts(sources);
    for source in sources.iter_mut() {
        apply_dir_slug_if_colliding(source, &counts);
    }
    let counts = segment_counts(sources);
    for (source, base) in sources.iter_mut().zip(base_segments) {
        apply_hash_if_colliding(source, &base, &counts);
    }
}

fn apply_dir_slug_if_colliding(source: &mut SkillSource, counts: &HashMap<String, usize>) {
    if counts
        .get(&source.link_segment)
        .copied()
        .unwrap_or(0)
        <= 1
    {
        return;
    }
    let Some((_, dir)) = local_dir_identity(&source.dep_path) else { return };
    let slug = local_dir_slug(&dir);
    if !slug.is_empty() {
        source.link_segment = format!("{}-{slug}", source.link_segment);
    }
}

fn apply_hash_if_colliding(source: &mut SkillSource, base: &str, counts: &HashMap<String, usize>) {
    if counts
        .get(&source.link_segment)
        .copied()
        .unwrap_or(0)
        <= 1
    {
        return;
    }
    if local_dir_identity(&source.dep_path).is_some() {
        source.link_segment = format!("{base}-{:08x}", fnv1a_32(&source.dep_path));
    }
}

fn segment_counts(sources: &[SkillSource]) -> HashMap<String, usize> {
    let mut counts = HashMap::with_capacity(sources.len());
    for source in sources {
        *counts.entry(source.link_segment.clone()).or_insert(0) += 1;
    }
    counts
}

fn local_dir_slug(dir: &str) -> String {
    dir.split('/')
        .filter(|part| !part.is_empty() && *part != "." && *part != "..")
        .map(sanitize_slug_part)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("+")
}

fn sanitize_slug_part(part: &str) -> String {
    part.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+' | '@') { ch } else { '+' }
        })
        .collect::<String>()
        .trim_matches('+')
        .to_string()
}

fn fnv1a_32(input: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in input.as_bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}
