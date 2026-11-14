use super::{Skill, SkillSource};
use crate::AGENT_SKILL_LINK_PREFIX;
use pnpm_crypto_hash::create_short_hash;
use pnpm_deps_restorer::{is_local_directory_version, parse_dep_path_name_version};
use std::collections::{HashMap, HashSet};

/// Maximum filesystem entry byte length (`NAME_MAX` on Linux, macOS, and Windows).
pub(in crate::agent_skills) const MAX_SKILL_ENTRY_BYTES: usize = 255;

/// Maximum byte length of a readable local directory slug before switching to a
/// digest suffix.
const MAX_DIR_SLUG_BYTES: usize = 48;

/// Hex characters taken from [`create_short_hash`] when shortening a segment.
const DIGEST_HEX_LEN: usize = 16;

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
fn is_link_dep_path(dep_path: &str) -> bool {
    parse_dep_path_name_version(dep_path).is_some_and(|(_, version)| version.starts_with("link:"))
}

/// The `(package_name, normalized_rel_dir)` of a `link:` or non-tarball `file:`
/// local directory `dep_path`.
fn local_dir_identity(dep_path: &str) -> Option<(&str, String)> {
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

/// Drop duplicate skills when both a `link:` dependency and an injected `file:`
/// copy of the same `(package_name, local_dir)` are approved, preferring the
/// live `link:` directory for any skill approved in both.
pub(in crate::agent_skills) fn dedup_approved_local_copies(approved: &mut Vec<SkillSource>) {
    let mut claimed: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for prefer_link in [true, false] {
        for source in approved.iter_mut() {
            if is_link_dep_path(&source.dep_path) != prefer_link {
                continue;
            }
            retain_unclaimed_local_skills(source, &mut claimed);
        }
    }
    approved.retain(|source| !source.skills.is_empty());
}

fn retain_unclaimed_local_skills(
    source: &mut SkillSource,
    claimed: &mut HashMap<(String, String), HashSet<String>>,
) {
    let Some((name, dir)) = local_dir_identity(&source.dep_path) else { return };
    let seen = claimed
        .entry((name.to_string(), dir))
        .or_default();
    source.skills.retain(|skill| seen.insert(skill.name.to_lowercase()));
}

/// Ensure local directory sources whose project alias collides with another
/// approved source receive a distinct, bounded `link_segment`, while keeping
/// single-source and registry/git segments unchanged.
pub(in crate::agent_skills) fn disambiguate_local_segments(sources: &mut [SkillSource]) {
    let base_segments: Vec<String> = sources
        .iter()
        .map(|source| source.link_segment.clone())
        .collect();
    let counts = segment_counts(sources);
    for (source, base) in sources.iter_mut().zip(&base_segments) {
        apply_dir_slug_or_hash_if_colliding(source, base, &counts);
    }
    let counts = segment_counts(sources);
    for (source, base) in sources.iter_mut().zip(&base_segments) {
        apply_hash_if_colliding(source, base, &counts);
    }
}

fn apply_dir_slug_or_hash_if_colliding(
    source: &mut SkillSource,
    base: &str,
    counts: &HashMap<String, usize>,
) {
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
    if !slug.is_empty() && slug.len() <= MAX_DIR_SLUG_BYTES {
        let candidate = format!("{base}-{slug}");
        if candidate.len() <= max_segment_bytes(&source.skills) {
            source.link_segment = candidate;
            return;
        }
    }
    source.link_segment = hashed_segment(base, &source.dep_path, &source.skills);
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
        source.link_segment = hashed_segment(base, &source.dep_path, &source.skills);
    }
}

fn max_segment_bytes(skills: &[Skill]) -> usize {
    let max_skill = skills
        .iter()
        .map(|skill| skill.name.len())
        .max()
        .unwrap_or(0);
    MAX_SKILL_ENTRY_BYTES.saturating_sub(AGENT_SKILL_LINK_PREFIX.len() + 1 + max_skill)
}

pub(in crate::agent_skills) fn hashed_segment(
    base: &str,
    dep_path: &str,
    skills: &[Skill],
) -> String {
    let digest = &create_short_hash(dep_path)[..DIGEST_HEX_LEN];
    let budget = max_segment_bytes(skills);
    if base.len() + 1 + DIGEST_HEX_LEN <= budget {
        return format!("{base}-{digest}");
    }
    if DIGEST_HEX_LEN < budget {
        let prefix = truncate_utf8(base, budget - 1 - DIGEST_HEX_LEN);
        if !prefix.is_empty() {
            return format!("{prefix}-{digest}");
        }
    }
    digest[..budget.min(DIGEST_HEX_LEN)].to_string()
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
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
        .collect()
}
