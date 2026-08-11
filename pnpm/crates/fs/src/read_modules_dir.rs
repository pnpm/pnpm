use std::{io, path::Path};

/// The package names directly under `modules_dir`, scoped names
/// included as `@scope/name`.
///
/// Counterpart of the TypeScript CLI's `readModulesDir`: dot-prefixed
/// entries (`.bin`, `.pnpm`, `.ignored`, tool caches such as `.cache`)
/// and plain files are not packages and are left out. Symlinks are
/// reported like any other entry — a `link:` dependency is a package as
/// far as this enumeration goes, and callers that must distinguish them
/// inspect the entry themselves.
///
/// A missing `modules_dir` yields an empty list; every other read
/// failure is surfaced.
pub fn read_modules_dir(modules_dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    collect_module_names(modules_dir, None, &mut names)?;
    Ok(names)
}

fn collect_module_names(
    modules_dir: &Path,
    scope: Option<&str>,
    names: &mut Vec<String>,
) -> io::Result<()> {
    let parent_dir = match scope {
        Some(scope) => modules_dir.join(scope),
        None => modules_dir.to_path_buf(),
    };
    let entries = match std::fs::read_dir(&parent_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            continue;
        }
        if scope.is_none() && name.starts_with('@') {
            collect_module_names(modules_dir, Some(name), names)?;
            continue;
        }
        match scope {
            Some(scope) => names.push(format!("{scope}/{name}")),
            None => names.push(name.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
