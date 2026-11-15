use super::{
    ConfiguredWorkspace, configured_workspace_state, lookup_inherited_workspace_entry,
    resolve_local_path,
};
use pnpm_matcher::WildcardMatcher;
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn workspace_includes_member(workspace_dir: &Path, member_dir: &Path) -> bool {
    let ws_dir = resolve_local_path(workspace_dir);
    let mem_dir = resolve_local_path(member_dir);
    let Ok(contents) = fs::read_to_string(ws_dir.join("Cargo.toml")) else {
        return false;
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return false;
    };
    let Some(ws) = doc.get("workspace").and_then(toml::Value::as_table) else {
        return false;
    };
    if ws_dir == mem_dir {
        return doc.contains_key("package");
    }
    let Some(rel_str) = relative_path_str(&ws_dir, &mem_dir) else {
        return false;
    };
    let excludes = toml_string_array(ws, "exclude");
    if is_excluded_path(&rel_str, &excludes) {
        return false;
    }
    let members = toml_string_array(ws, "members");
    if members.iter().any(|pat| cargo_glob_matches(pat, &rel_str)) {
        return true;
    }
    is_implicit_workspace_member(&ws_dir, &mem_dir, &doc, &members, &excludes)
}

fn relative_path_str(base: &Path, target: &Path) -> Option<String> {
    let rel = pnpm_fs::relative_path(base, target);
    (!rel.is_absolute() && !rel.as_os_str().is_empty()).then(|| {
        rel.to_string_lossy().replace('\\', "/")
    })
}

fn is_excluded_path(rel_str: &str, excludes: &[String]) -> bool {
    excludes
        .iter()
        .any(|pat| {
            let clean = pat.trim_start_matches("./").trim_matches('/');
            !clean.is_empty()
                && (rel_str == clean
                    || rel_str.starts_with(&format!("{clean}/"))
                    || cargo_glob_matches(clean, rel_str))
        })
}

fn is_implicit_workspace_member(
    ws_dir: &Path,
    mem_dir: &Path,
    root_doc: &toml::Table,
    members: &[String],
    excludes: &[String],
) -> bool {
    if !mem_dir.join("Cargo.toml").is_file() {
        return false;
    }
    let mut visited = HashSet::new();
    let mut queue = seed_workspace_packages(ws_dir, root_doc, members, excludes);
    while let Some(pkg_dir) = queue.pop_front() {
        if !visited.insert(pkg_dir.clone()) {
            continue;
        }
        for dep_dir in eligible_path_dependencies(&pkg_dir, ws_dir, root_doc, excludes) {
            if dep_dir == *mem_dir {
                return true;
            }
            queue.push_back(dep_dir);
        }
    }
    false
}

fn seed_workspace_packages(
    ws_dir: &Path,
    root_doc: &toml::Table,
    members: &[String],
    excludes: &[String],
) -> VecDeque<PathBuf> {
    let mut seeds = VecDeque::new();
    if root_doc.contains_key("package") {
        seeds.push_back(ws_dir.to_path_buf());
    }
    let mut candidate_dirs = Vec::new();
    collect_cargo_dirs(ws_dir, ws_dir, excludes, &mut candidate_dirs);
    for dir in candidate_dirs {
        if let Some(rel_str) = relative_path_str(ws_dir, &dir)
            && members.iter().any(|pat| cargo_glob_matches(pat, &rel_str))
        {
            seeds.push_back(dir);
        }
    }
    seeds
}

fn collect_cargo_dirs(
    ws_dir: &Path,
    current_dir: &Path,
    excludes: &[String],
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = resolve_local_path(&entry.path());
        let is_real_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        if !is_real_dir || is_skipped_dir(&path, ws_dir, excludes) {
            continue;
        }
        if path.join("Cargo.toml").is_file() {
            out.push(path.clone());
        }
        collect_cargo_dirs(ws_dir, &path, excludes, out);
    }
}

fn is_skipped_dir(path: &Path, ws_dir: &Path, excludes: &[String]) -> bool {
    let Some(name) = path.file_name().and_then(|os| os.to_str()) else {
        return true;
    };
    if matches!(name, "target" | "node_modules" | ".git") {
        return true;
    }
    relative_path_str(ws_dir, path).is_none_or(|rel_str| is_excluded_path(&rel_str, excludes))
}

fn eligible_path_dependencies(
    pkg_dir: &Path,
    ws_dir: &Path,
    root_doc: &toml::Table,
    excludes: &[String],
) -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(pkg_dir.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_table_path_deps(&doc, pkg_dir, (ws_dir, root_doc), &mut paths);
    paths
        .into_iter()
        .filter(|dep_dir| is_eligible_implicit_member(dep_dir, ws_dir, excludes))
        .collect()
}

fn is_eligible_implicit_member(dep_dir: &Path, ws_dir: &Path, excludes: &[String]) -> bool {
    if !dep_dir.join("Cargo.toml").is_file() {
        return false;
    }
    let Some(rel) = relative_path_str(ws_dir, dep_dir) else {
        return false;
    };
    if is_excluded_path(&rel, excludes) {
        return false;
    }
    match configured_workspace_state(dep_dir) {
        ConfiguredWorkspace::Path(cfg_ws) => cfg_ws == ws_dir,
        ConfiguredWorkspace::Invalid => false,
        ConfiguredWorkspace::Unset => dep_dir.starts_with(ws_dir),
    }
}

fn collect_table_path_deps(
    doc: &toml::Table,
    pkg_dir: &Path,
    workspace_ctx: (&Path, &toml::Table),
    out: &mut Vec<PathBuf>,
) {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get(section).and_then(toml::Value::as_table) {
            extract_paths_from_dep_table(table, pkg_dir, workspace_ctx, out);
        }
    }
    let Some(targets) = doc.get("target").and_then(toml::Value::as_table) else {
        return;
    };
    for target_cfg in targets.values().filter_map(toml::Value::as_table) {
        collect_table_path_deps(target_cfg, pkg_dir, workspace_ctx, out);
    }
}

fn extract_paths_from_dep_table(
    table: &toml::Table,
    pkg_dir: &Path,
    (ws_dir, root_doc): (&Path, &toml::Table),
    out: &mut Vec<PathBuf>,
) {
    for (key, value) in table {
        let Some(dep) = value.as_table() else {
            continue;
        };
        if let Some(rel_path) = dep.get("path").and_then(toml::Value::as_str) {
            out.push(resolve_local_path(&pkg_dir.join(rel_path)));
        } else if dep.get("workspace").and_then(toml::Value::as_bool) == Some(true)
            && let Some(ws_path) = resolve_workspace_dep_path(ws_dir, root_doc, key, dep)
        {
            out.push(ws_path);
        }
    }
}

fn resolve_workspace_dep_path(
    ws_dir: &Path,
    root_doc: &toml::Table,
    key: &str,
    member_dep: &toml::Table,
) -> Option<PathBuf> {
    let ws_deps = root_doc
        .get("workspace")?
        .as_table()?
        .get("dependencies")?
        .as_table()?;
    let dep_package = member_dep.get("package").and_then(toml::Value::as_str);
    let (_, ws_val) =
        lookup_inherited_workspace_entry(key, dep_package, |name| ws_deps.get_key_value(name))?;
    let rel_path = ws_val
        .as_table()?
        .get("path")?
        .as_str()?;
    Some(resolve_local_path(&ws_dir.join(rel_path)))
}

fn toml_string_array(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn cargo_glob_matches(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern
        .trim_start_matches("./")
        .trim_matches('/')
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect();
    let path_segs: Vec<&str> = path
        .split('/')
        .filter(|seg| !seg.is_empty())
        .collect();
    match_segments(&pat_segs, &path_segs)
}

fn match_segments(pat: &[&str], path: &[&str]) -> bool {
    let mut reachable = vec![false; path.len() + 1];
    reachable[0] = true;
    for &pat_seg in pat {
        advance_segment(&mut reachable, pat_seg, path);
    }
    reachable[path.len()]
}

fn advance_segment(reachable: &mut [bool], pat_seg: &str, path: &[&str]) {
    if pat_seg == "**" {
        for idx in 1..=path.len() {
            reachable[idx] |= reachable[idx - 1] && path[idx - 1] != "..";
        }
        return;
    }
    if pat_seg == ".." {
        for idx in (1..=path.len()).rev() {
            reachable[idx] = reachable[idx - 1] && path[idx - 1] == "..";
        }
        reachable[0] = false;
        return;
    }
    let matcher = WildcardMatcher::new(pat_seg);
    for idx in (1..=path.len()).rev() {
        reachable[idx] =
            reachable[idx - 1] && path[idx - 1] != ".." && matcher.matches(path[idx - 1]);
    }
    reachable[0] = false;
}
