use super::{
    EditDirState, PatchCommitError, Path, PathBuf, patched_identity, read_all_edit_dir_states,
    read_edit_dir_state, resolve_path,
};
use pnpm_resolving_parse_wanted_dependency::{ParsedWantedDependency, parse_wanted_dependency};
use std::collections::BTreeMap;

pub(super) struct ResolvedPatchDir {
    pub(super) patch_dir: PathBuf,
    pub(super) state_value: EditDirState,
}

pub(super) fn resolve_patch_dir(
    dir: &Path,
    modules_dir: &Path,
    user_param: &Path,
) -> Result<ResolvedPatchDir, PatchCommitError> {
    if let Some(resolved) = resolve_direct_path(dir, modules_dir, user_param)? {
        return Ok(resolved);
    }

    let all_states = read_all_edit_dir_states(modules_dir).map_err(PatchCommitError::StateFile)?;
    if !all_states.is_empty() {
        let query_str = user_param.to_string_lossy();
        let query = PatchQuery::new(&query_str);
        let buckets = collect_matches(all_states, modules_dir, &query);
        if let Some(resolved) = pick_candidate(query_str.into_owned(), buckets)? {
            return Ok(resolved);
        }
    }

    fallback_direct_path(modules_dir, resolve_path(dir, user_param))
}

fn resolve_direct_path(
    dir: &Path,
    modules_dir: &Path,
    user_param: &Path,
) -> Result<Option<ResolvedPatchDir>, PatchCommitError> {
    let direct_path = resolve_path(dir, user_param);
    if direct_path.is_dir()
        && let Some(state_value) =
            read_edit_dir_state(modules_dir, &direct_path).map_err(PatchCommitError::StateFile)?
    {
        let _ = patched_identity(&direct_path)?;
        return Ok(Some(ResolvedPatchDir { patch_dir: direct_path, state_value }));
    }
    Ok(None)
}

fn fallback_direct_path(
    modules_dir: &Path,
    direct_path: PathBuf,
) -> Result<ResolvedPatchDir, PatchCommitError> {
    let state_value = read_edit_dir_state(modules_dir, &direct_path)
        .map_err(PatchCommitError::StateFile)?
        .ok_or_else(|| PatchCommitError::InvalidPatchDir { patch_dir: direct_path.clone() })?;
    let _ = patched_identity(&direct_path)?;

    Ok(ResolvedPatchDir { patch_dir: direct_path, state_value })
}

enum MatchKind {
    None,
    NameOnly,
    Exact,
}

struct PatchQuery<'a> {
    raw: &'a str,
    parsed: ParsedWantedDependency,
}

impl<'a> PatchQuery<'a> {
    fn new(raw: &'a str) -> Self {
        Self { raw, parsed: parse_wanted_dependency(raw) }
    }

    fn matches_candidate(
        &self,
        candidate_path: &Path,
        patches_dir: &Path,
        state_value: &EditDirState,
        name: &str,
        version: &str,
    ) -> MatchKind {
        if self.is_exact_match(candidate_path, patches_dir, state_value, name, version) {
            return MatchKind::Exact;
        }
        if self.is_name_match(name, state_value) {
            return MatchKind::NameOnly;
        }
        MatchKind::None
    }

    fn is_exact_match(
        &self,
        candidate_path: &Path,
        patches_dir: &Path,
        state_value: &EditDirState,
        name: &str,
        version: &str,
    ) -> bool {
        if self.parsed.bare_specifier.is_some() && state_value.patched_pkg == self.raw {
            return true;
        }
        if self.matches_specifier(name, version) {
            return true;
        }
        if candidate_path.file_name().and_then(|file_name| file_name.to_str()) == Some(self.raw) {
            return true;
        }
        candidate_path
            .strip_prefix(patches_dir)
            .is_ok_and(|relative| relative.to_string_lossy() == self.raw)
    }

    fn matches_specifier(&self, name: &str, version: &str) -> bool {
        if format!("{name}@{version}") == self.raw {
            return true;
        }
        let Some(alias) = &self.parsed.alias else { return false };
        if alias != name {
            return false;
        }
        let Some(spec) = &self.parsed.bare_specifier else { return false };
        if spec == version {
            return true;
        }
        let (Ok(range), Ok(ver)) =
            (spec.parse::<node_semver::Range>(), version.parse::<node_semver::Version>())
        else {
            return false;
        };
        range.satisfies(&ver)
    }

    fn is_name_match(&self, name: &str, state_value: &EditDirState) -> bool {
        if self.parsed.bare_specifier.is_some() {
            return false;
        }
        name == self.raw
            || state_value.patched_pkg == self.raw
            || self.parsed.alias.as_deref() == Some(name)
    }
}

struct MatchBuckets {
    exact: Vec<(PathBuf, EditDirState)>,
    name: Vec<(PathBuf, EditDirState)>,
}

fn collect_matches(
    all_states: BTreeMap<String, EditDirState>,
    modules_dir: &Path,
    query: &PatchQuery<'_>,
) -> MatchBuckets {
    let patches_dir = modules_dir.join(".pnpm_patches");
    let mut buckets = MatchBuckets { exact: Vec::new(), name: Vec::new() };

    for (edit_dir_str, state_value) in all_states {
        let candidate_path = PathBuf::from(edit_dir_str);
        if !candidate_path.is_dir() {
            continue;
        }
        let Ok((name, version)) = patched_identity(&candidate_path) else {
            continue;
        };
        match query.matches_candidate(&candidate_path, &patches_dir, &state_value, &name, &version)
        {
            MatchKind::Exact => buckets.exact.push((candidate_path, state_value)),
            MatchKind::NameOnly => buckets.name.push((candidate_path, state_value)),
            MatchKind::None => {}
        }
    }

    buckets
}

fn pick_candidate(
    query_str: String,
    buckets: MatchBuckets,
) -> Result<Option<ResolvedPatchDir>, PatchCommitError> {
    if buckets.exact.len() == 1 {
        let (patch_dir, state_value) = buckets.exact
            .into_iter()
            .next()
            .unwrap();
        return Ok(Some(ResolvedPatchDir { patch_dir, state_value }));
    }
    if buckets.exact.len() > 1 {
        let candidates = buckets.exact
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        return Err(PatchCommitError::AmbiguousPatchTarget { query: query_str, candidates });
    }
    if buckets.name.len() == 1 {
        let (patch_dir, state_value) = buckets.name.into_iter().next().unwrap();
        return Ok(Some(ResolvedPatchDir { patch_dir, state_value }));
    }
    if buckets.name.len() > 1 {
        let candidates = buckets.name
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        return Err(PatchCommitError::AmbiguousPatchTarget { query: query_str, candidates });
    }
    Ok(None)
}

pub(super) fn format_candidates(candidates: &[PathBuf]) -> String {
    let mut sorted = candidates.to_vec();
    sorted.sort();
    sorted
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}
