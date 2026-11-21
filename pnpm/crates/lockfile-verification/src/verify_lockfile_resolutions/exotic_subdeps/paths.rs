pub(super) fn split_absolute_prefix(path: &str) -> (Option<String>, &str) {
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
    {
        let drive = char::from(bytes[0].to_ascii_uppercase());
        return (Some(format!("{drive}:/")), &path[3..]);
    }
    if let Some(stripped) = path.strip_prefix('/') {
        return (Some("/".to_string()), stripped);
    }
    (None, path)
}

pub(super) fn normalize_posix_rel_path(path: &str) -> String {
    let (abs_prefix, remainder) = split_absolute_prefix(path);
    let is_abs = abs_prefix.is_some();
    let mut parts: Vec<&str> = Vec::new();
    for segment in remainder.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => pop_or_keep_parent_segment(&mut parts, is_abs),
            seg => parts.push(seg),
        }
    }
    if let Some(prefix) = abs_prefix {
        format!("{prefix}{}", parts.join("/"))
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn pop_or_keep_parent_segment(parts: &mut Vec<&str>, is_abs: bool) {
    if parts
        .last()
        .is_some_and(|last| *last != "..")
    {
        parts.pop();
    } else if !is_abs {
        parts.push("..");
    }
}
