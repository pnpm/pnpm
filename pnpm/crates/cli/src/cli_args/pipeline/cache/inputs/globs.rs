use super::{
    FileIterator, Glob, HashedFile, IntoDiagnostic, Path, PathBuf, Program, TaskCache,
    check_ancestors, check_input_directories, io, is_excluded_input, validate_relative_path,
};
use std::path::Component;
use wax::walk::{Entry, GlobEntry, WalkError};

pub(super) enum InputSelection<'a> {
    Default,
    Empty,
    Configured(ConfiguredInputs<'a>),
}

pub(super) struct ConfiguredInputs<'a> {
    pub(super) replace: Vec<Glob<'a>>,
    pub(super) add: Vec<ParentGlob>,
    pub(super) exclude: Vec<ParentGlob>,
}

pub(super) struct ParentGlob {
    raw: String,
    parent_depth: usize,
    literal_dir_prefix: PathBuf,
    unanchored: bool,
    glob: Glob<'static>,
}

pub(super) fn parse_inputs(inputs: Option<&[String]>) -> miette::Result<InputSelection<'_>> {
    let Some(patterns) = inputs else {
        return Ok(InputSelection::Default);
    };
    if patterns.is_empty() {
        return Ok(InputSelection::Empty);
    }
    let mut replace = Vec::new();
    let mut add = Vec::new();
    let mut exclude = Vec::new();
    for pattern in patterns {
        if let Some(rest) = pattern.strip_prefix('+') {
            add.push(ParentGlob::parse(pattern, rest)?);
        } else if let Some(rest) = pattern.strip_prefix('!') {
            exclude.push(ParentGlob::parse(pattern, rest)?);
        } else {
            replace.push(compile_replace_glob(pattern)?);
        }
    }
    Ok(InputSelection::Configured(ConfiguredInputs { replace, add, exclude }))
}

fn compile_replace_glob(pattern: &str) -> miette::Result<Glob<'_>> {
    let trimmed = strip_cur_dir_prefixes(pattern);
    validate_glob_body(pattern, trimmed)?;
    let glob =
        Glob::new(trimmed).map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))?;
    if !matches!(glob.has_root(), wax::query::When::Never) {
        return Err(miette::miette!("cache input glob must be relative: {pattern:?}"));
    }
    Ok(glob)
}

impl ParentGlob {
    fn parse(raw: &str, body: &str) -> miette::Result<Self> {
        let (parent_depth, rest) = split_parent_segments(body);
        validate_glob_body(raw, rest)?;
        let glob = Glob::new(rest)
            .map(Glob::into_owned)
            .map_err(|error| miette::miette!("invalid glob {raw:?}: {error}"))?;
        if !matches!(glob.has_root(), wax::query::When::Never) {
            return Err(miette::miette!("cache input glob must be relative: {raw:?}"));
        }
        Ok(Self {
            raw: raw.to_string(),
            parent_depth,
            literal_dir_prefix: literal_directory_prefix(rest),
            unanchored: rest == "**" || rest.starts_with("**/"),
            glob,
        })
    }

    pub(super) fn matches_rel_path(&self, project: &Path, rel_path: &str) -> bool {
        let (file_up, file_tail) = split_parent_segments(rel_path);
        if let Some(rebased) = rebase_to_ancestor(project, file_up, file_tail, self.parent_depth) {
            return self.glob.is_match(rebased.as_str());
        }
        self.parent_depth == 0 && self.unanchored && self.glob.is_match(file_tail)
    }
}

fn rebase_to_ancestor(
    project: &Path,
    file_up: usize,
    file_tail: &str,
    excl_up: usize,
) -> Option<String> {
    if excl_up < file_up {
        return None;
    }
    if excl_up == file_up {
        return Some(file_tail.to_string());
    }
    let file_root = project.ancestors().nth(file_up)?;
    let excl_root = file_root
        .ancestors()
        .nth(excl_up - file_up)?;
    let prefix = file_root.strip_prefix(excl_root).ok()?;
    let mut out = String::new();
    for component in prefix.components() {
        let Component::Normal(segment) = component else { return None };
        out.push_str(segment.to_str()?);
        out.push('/');
    }
    out.push_str(file_tail);
    Some(out)
}

fn strip_cur_dir_prefixes(mut pattern: &str) -> &str {
    while let Some(tail) = pattern.strip_prefix("./") {
        pattern = tail;
    }
    pattern
}

fn split_parent_segments(pattern: &str) -> (usize, &str) {
    let mut trimmed = strip_cur_dir_prefixes(pattern);
    let mut parent_depth = 0usize;
    while let Some(tail) = trimmed.strip_prefix("../") {
        parent_depth += 1;
        trimmed = strip_cur_dir_prefixes(tail);
    }
    (parent_depth, trimmed)
}

fn validate_glob_body(raw: &str, body: &str) -> miette::Result<()> {
    if body.is_empty() || body.starts_with(['/', '\\']) || Path::new(body).has_root() {
        return Err(miette::miette!("invalid cache input glob {raw:?}"));
    }
    let invalid_segment = body.split('/').any(str::is_empty)
        || body
            .split(['/', '\\'])
            .any(is_disallowed_glob_segment);
    if invalid_segment {
        return Err(miette::miette!(
            "cache input glob cannot contain '.', '..', '.git', or 'node_modules': {raw:?}",
        ));
    }
    Ok(())
}

fn is_disallowed_glob_segment(segment: &str) -> bool {
    segment == "."
        || segment == ".."
        || segment.eq_ignore_ascii_case(".git")
        || segment.eq_ignore_ascii_case("node_modules")
}

fn literal_directory_prefix(body: &str) -> PathBuf {
    let segments: Vec<&str> = body.split('/').collect();
    let literal_count = segments
        .iter()
        .take_while(|segment| !segment.chars().any(wax::is_meta_character))
        .count();
    let dir_count = if literal_count < segments.len() {
        literal_count
    } else {
        literal_count.saturating_sub(1)
    };
    segments[..dir_count].iter().collect()
}

struct AdditiveWalk<'a> {
    cache: &'a TaskCache,
    project: &'a Path,
    output_globs: &'a [Glob<'a>],
    exclude: &'a [ParentGlob],
}

pub(super) fn walk_additive_inputs(
    cache: &TaskCache,
    project: &Path,
    output_globs: &[Glob<'_>],
    configured: &ConfiguredInputs<'_>,
) -> miette::Result<Vec<HashedFile>> {
    let walk = AdditiveWalk { cache, project, output_globs, exclude: &configured.exclude };
    let mut files = Vec::new();
    for glob in &configured.add {
        walk_single_additive_glob(&walk, glob, &mut files)?;
    }
    Ok(files)
}

fn walk_single_additive_glob(
    walk: &AdditiveWalk<'_>,
    glob: &ParentGlob,
    files: &mut Vec<HashedFile>,
) -> miette::Result<()> {
    let walk_root = resolve_walk_root(&walk.cache.workspace_root, walk.project, glob)?;
    let entries = glob.glob
        .walk(&walk_root)
        .not(wax::any(["**/node_modules/**", "**/.git/**"]))
        .map_err(|error| miette::miette!("invalid ignore glob: {error}"))?;
    for entry in entries {
        if let Some(hashed) = hash_walk_entry(walk, &walk_root, &glob.raw, entry)? {
            files.push(hashed);
        }
    }
    Ok(())
}

fn resolve_walk_root(
    workspace_root: &Path,
    project: &Path,
    glob: &ParentGlob,
) -> miette::Result<PathBuf> {
    check_ancestors(project, Path::new("")).into_diagnostic()?;
    let mut walk_root = project;
    for _ in 0..glob.parent_depth {
        walk_root = walk_root
            .parent()
            .ok_or_else(|| {
                let root_display = workspace_root.display();
                miette::miette!(
                    "cache input glob {:?} escapes workspace root {root_display}",
                    glob.raw,
                )
            })?;
    }
    verify_workspace_containment(workspace_root, project, walk_root, glob)?;
    Ok(walk_root.to_path_buf())
}

fn verify_workspace_containment(
    workspace_root: &Path,
    project: &Path,
    walk_root: &Path,
    glob: &ParentGlob,
) -> miette::Result<()> {
    let canonical_workspace = dunce::canonicalize(workspace_root).into_diagnostic()?;
    let canonical_walk = dunce::canonicalize(walk_root).into_diagnostic()?;
    let Ok(rel_to_workspace) = canonical_walk.strip_prefix(&canonical_workspace) else {
        let root_display = workspace_root.display();
        return Err(miette::miette!(
            "cache input glob {:?} escapes workspace root {root_display}",
            glob.raw,
        ));
    };
    check_ancestors(workspace_root, rel_to_workspace).into_diagnostic()?;
    if let Ok(lexical_rel) = walk_root.strip_prefix(workspace_root) {
        check_ancestors(workspace_root, lexical_rel).into_diagnostic()?;
    }
    if let Ok(project_rel) = project.strip_prefix(walk_root) {
        check_ancestors(walk_root, project_rel).into_diagnostic()?;
    }
    if !glob.literal_dir_prefix.as_os_str().is_empty() {
        check_ancestors(walk_root, &glob.literal_dir_prefix).into_diagnostic()?;
    }
    Ok(())
}

fn hash_walk_entry(
    walk: &AdditiveWalk<'_>,
    walk_root: &Path,
    raw_pattern: &str,
    entry: Result<GlobEntry, WalkError>,
) -> miette::Result<Option<HashedFile>> {
    let entry = match entry {
        Ok(entry) => entry,
        Err(walk_error) => return handle_walk_error(raw_pattern, walk.project, walk_error),
    };
    if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
        return Ok(None);
    }
    let entry_path = entry.into_path();
    let (entry_rel, rel_path) = relative_from_project(walk.project, walk_root, &entry_path)?;
    if is_excluded_input(walk.project, &rel_path, walk.output_globs, walk.exclude) {
        return Ok(None);
    }
    check_input_directories(walk_root, entry_rel).into_diagnostic()?;
    let hash = walk.cache
        .hash_cached_file(&entry_path)
        .map_err(|error| {
            let project_display = walk.project.display();
            miette::miette!("hashing cache input {rel_path:?} in {project_display}: {error}")
        })?;
    Ok(hash.map(|hash| HashedFile { rel_path, hash }))
}

fn handle_walk_error(
    raw_pattern: &str,
    project: &Path,
    walk_error: WalkError,
) -> miette::Result<Option<HashedFile>> {
    let io_error = io::Error::from(walk_error);
    if io_error.kind() == io::ErrorKind::NotFound {
        return Ok(None);
    }
    let project_display = project.display();
    Err(miette::miette!(
        "walking cache input glob {raw_pattern:?} in {project_display}: {io_error}",
    ))
}

fn relative_from_project<'a>(
    project: &Path,
    walk_root: &Path,
    entry_path: &'a Path,
) -> miette::Result<(&'a Path, String)> {
    let entry_rel = entry_path
        .strip_prefix(walk_root)
        .map_err(|_| {
            let root_display = walk_root.display();
            let entry_display = entry_path.display();
            miette::miette!("cache input path must stay under {root_display}: {entry_display}")
        })?;
    validate_relative_path(entry_rel).into_diagnostic()?;
    let project_rel = project.strip_prefix(walk_root).expect("walk_root is an ancestor of project");
    let diff = pathdiff::diff_paths(entry_rel, project_rel).expect("both paths are relative");
    let rel_path = relative_components_to_slash_path(&diff, project)?;
    Ok((entry_rel, rel_path))
}

fn relative_components_to_slash_path(path: &Path, display_root: &Path) -> miette::Result<String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => parts.push(".."),
            Component::Normal(segment) => {
                let text = segment
                    .to_str()
                    .ok_or_else(|| {
                        let root_display = display_root.display();
                        miette::miette!("non-UTF-8 cache input path {path:?} in {root_display}")
                    })?;
                parts.push(text);
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                return Err(miette::miette!("cache input path must be relative: {path:?}"));
            }
        }
    }
    Ok(parts.join("/"))
}
