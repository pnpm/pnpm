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
    let entries = match std::fs::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(names),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let Some((name, file_type)) = parse_package_entry(&entry?)? else {
            continue;
        };
        if name.starts_with('@') {
            // Names below a symlinked scope container reach their target
            // through the symlink, wherever it points — a caller that deletes
            // what it enumerates follows it out of `modules_dir`. pnpm only
            // ever symlinks the packages inside a scope, never the scope
            // itself, so skipping costs nothing.
            if !file_type.is_symlink() {
                collect_scoped_module_names(modules_dir, &name, &mut names)?;
            }
        } else {
            names.push(name);
        }
    }
    Ok(names)
}

fn collect_scoped_module_names(
    modules_dir: &Path,
    scope: &str,
    names: &mut Vec<String>,
) -> io::Result<()> {
    let entries = match std::fs::read_dir(modules_dir.join(scope)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        if let Some((name, _)) = parse_package_entry(&entry?)? {
            names.push(format!("{scope}/{name}"));
        }
    }
    Ok(())
}

fn parse_package_entry(
    entry: &std::fs::DirEntry,
) -> io::Result<Option<(String, std::fs::FileType)>> {
    let file_name = entry.file_name();
    let Some(name) = file_name.to_str() else {
        return Ok(None);
    };
    if name.starts_with('.') {
        return Ok(None);
    }
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        // The entry was removed between the directory read and this call.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if file_type.is_file() {
        return Ok(None);
    }
    Ok(Some((name.to_string(), file_type)))
}

#[cfg(test)]
mod tests;
