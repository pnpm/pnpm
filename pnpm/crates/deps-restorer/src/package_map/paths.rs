use pnpm_fs::lexical_normalize;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

pub(super) fn to_relative_url(from: &Path, to: &Path) -> String {
    let Some(relative) = pathdiff::diff_paths(to, from) else {
        return absolute_package_url(to);
    };
    let relative = normalize_path(&relative);
    let relative = if relative.is_empty() {
        ".".to_string()
    } else {
        relative
    };
    if relative == "."
        || relative == ".."
        || relative.starts_with("./")
        || relative.starts_with("../")
    {
        relative
    } else {
        format!("./{relative}")
    }
}

pub(super) fn link_target_id(relative: Option<PathBuf>, dir: &Path) -> String {
    let Some(relative) = relative else {
        return format!("link:{}", normalize_path(dir));
    };
    let relative_id = normalize_path(&relative);
    if relative_id == ".." || relative_id.starts_with("../") {
        format!("link:{}", normalize_path(dir))
    } else if relative_id.is_empty() {
        ".".to_string()
    } else {
        relative_id
    }
}

pub(super) fn graph_package_id(package_dir: &Path, modules_dir: &Path) -> String {
    let package_dir = lexical_normalize(package_dir);
    let Some(relative) = pathdiff::diff_paths(&package_dir, modules_dir) else {
        return format!("link:{}", normalize_path(&package_dir));
    };
    let relative = normalize_path(&relative);
    if relative == ".." || relative.is_empty() {
        ".".to_string()
    } else {
        relative
    }
}

pub(super) fn absolute_package_url(path: &Path) -> String {
    let normalized = normalize_path(path);
    if cfg!(windows) && normalized.starts_with("//") {
        format!("file:{}", encode_url_path(&normalized))
    } else if cfg!(windows) && !normalized.starts_with('/') {
        format!("file:///{}", encode_url_path(&normalized))
    } else {
        format!("file://{}", encode_url_path(&normalized))
    }
}

fn encode_url_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                encoded.push(byte as char);
            }
            _ => write!(encoded, "%{byte:02X}").expect("writing to a string cannot fail"),
        }
    }
    encoded
}

pub(super) fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
