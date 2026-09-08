use std::{io, path::Path};

/// The package names directly under `modules_dir`, scoped names
/// included as `@scope/name`.
///
/// Counterpart of the TypeScript CLI's `readModulesDir`: dot-prefixed
/// entries (`.bin`, `.pnpm`, `.ignored`, tool caches such as `.cache`)
/// and plain files are not packages and are left out. A symlinked
/// *package* is reported like any other — a `link:` dependency is a
/// package as far as this enumeration goes, and callers that must
/// distinguish them inspect the entry themselves. A symlinked *scope
/// container* is not, since the names below it reach their target
/// through the symlink rather than through `modules_dir`.
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
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            // The entry was removed between the directory read and this call.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if file_type.is_file() {
            continue;
        }
        if scope.is_none() && name.starts_with('@') {
            // Names below a symlinked scope container reach their target
            // through the symlink, wherever it points — a caller that deletes
            // what it enumerates follows it out of `modules_dir`. pnpm only
            // ever symlinks the packages inside a scope, never the scope
            // itself, so skipping costs nothing.
            if file_type.is_symlink() {
                continue;
            }
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
