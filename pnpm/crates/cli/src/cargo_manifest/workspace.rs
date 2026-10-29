use pnpm_matcher::WildcardMatcher;
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
};

pub(crate) fn workspace_includes_member(workspace_dir: &Path, member_dir: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(workspace_dir.join("Cargo.toml")) else {
        return false;
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return false;
    };
    let Some(ws) = doc.get("workspace").and_then(toml::Value::as_table) else {
        return false;
    };
    let Some(rel_str) = relative_member_path(workspace_dir, member_dir) else {
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
    is_implicit_workspace_member(workspace_dir, member_dir, &doc, &members, &excludes)
}

fn relative_member_path(workspace_dir: &Path, member_dir: &Path) -> Option<String> {
    let rel = member_dir.strip_prefix(workspace_dir).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
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
    workspace_dir: &Path,
    member_dir: &Path,
    root_doc: &toml::Table,
    members: &[String],
    excludes: &[String],
) -> bool {
    if !member_dir.join("Cargo.toml").is_file() {
        return false;
    }
    let target = normalize_lexical(member_dir);
    let mut visited = HashSet::new();
    let mut queue = seed_workspace_packages(workspace_dir, root_doc, members, excludes);
    while let Some(pkg_dir) = queue.pop_front() {
        if !visited.insert(pkg_dir.clone()) {
            continue;
        }
        for dep_dir in eligible_path_dependencies(&pkg_dir, workspace_dir, excludes) {
            if dep_dir == target {
                return true;
            }
            queue.push_back(dep_dir);
        }
    }
    false
}

fn seed_workspace_packages(
    workspace_dir: &Path,
    root_doc: &toml::Table,
    members: &[String],
    excludes: &[String],
) -> VecDeque<PathBuf> {
    let mut seeds = VecDeque::new();
    if root_doc.contains_key("package") {
        seeds.push_back(normalize_lexical(workspace_dir));
    }
    let mut candidate_dirs = Vec::new();
    collect_cargo_dirs(workspace_dir, workspace_dir, excludes, &mut candidate_dirs);
    for dir in candidate_dirs {
        if let Some(rel_str) = relative_member_path(workspace_dir, &dir)
            && members.iter().any(|pat| cargo_glob_matches(pat, &rel_str))
        {
            seeds.push_back(normalize_lexical(&dir));
        }
    }
    seeds
}

fn collect_cargo_dirs(
    workspace_dir: &Path,
    current_dir: &Path,
    excludes: &[String],
    out: &mut Vec<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_real_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        if !is_real_dir || is_skipped_dir(&path, workspace_dir, excludes) {
            continue;
        }
        if path.join("Cargo.toml").is_file() {
            out.push(path.clone());
        }
        collect_cargo_dirs(workspace_dir, &path, excludes, out);
    }
}

fn is_skipped_dir(path: &Path, workspace_dir: &Path, excludes: &[String]) -> bool {
    let Some(name) = path.file_name().and_then(|os| os.to_str()) else {
        return true;
    };
    if matches!(name, "target" | "node_modules" | ".git") {
        return true;
    }
    relative_member_path(workspace_dir, path)
        .is_none_or(|rel_str| is_excluded_path(&rel_str, excludes))
}

fn eligible_path_dependencies(
    pkg_dir: &Path,
    workspace_dir: &Path,
    excludes: &[String],
) -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(pkg_dir.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_table_path_deps(&doc, pkg_dir, &mut paths);
    let norm_ws = normalize_lexical(workspace_dir);
    paths
        .into_iter()
        .filter(|dep_dir| {
            dep_dir.join("Cargo.toml").is_file()
                && relative_member_path(&norm_ws, dep_dir)
                    .is_some_and(|rel| !is_excluded_path(&rel, excludes))
        })
        .collect()
}

fn collect_table_path_deps(doc: &toml::Table, pkg_dir: &Path, out: &mut Vec<PathBuf>) {
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = doc.get(section).and_then(toml::Value::as_table) {
            extract_paths_from_dep_table(table, pkg_dir, out);
        }
    }
    let Some(targets) = doc.get("target").and_then(toml::Value::as_table) else {
        return;
    };
    for target_cfg in targets.values().filter_map(toml::Value::as_table) {
        collect_table_path_deps(target_cfg, pkg_dir, out);
    }
}

fn extract_paths_from_dep_table(table: &toml::Table, pkg_dir: &Path, out: &mut Vec<PathBuf>) {
    for value in table.values() {
        if let Some(rel_path) = value
            .as_table()
            .and_then(|dep| dep.get("path"))
            .and_then(toml::Value::as_str)
        {
            out.push(normalize_lexical(&pkg_dir.join(rel_path)));
        }
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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
            reachable[idx] |= reachable[idx - 1];
        }
        return;
    }
    let matcher = WildcardMatcher::new(pat_seg);
    for idx in (1..=path.len()).rev() {
        reachable[idx] = reachable[idx - 1] && matcher.matches(path[idx - 1]);
    }
    reachable[0] = false;
}
