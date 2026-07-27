use regex::Regex;
use std::{
    collections::HashSet,
    io::{self, ErrorKind},
    path::Path,
    sync::OnceLock,
};

const LICENSE_FILES: &[&str] = &[
    "LICENSE",
    "LICENCE",
    "LICENSE.md",
    "LICENCE.md",
    "LICENSE.txt",
    "LICENCE.txt",
    "MIT-LICENSE.txt",
    "MIT-LICENSE.md",
    "MIT-LICENSE",
];

const LICENSE_NAMES: &[&str] = &[
    "Apache1_1",
    "Apache-1.1",
    "Apache 1.1",
    "Apache2",
    "Apache-2.0",
    "Apache 2.0",
    "BSD",
    "BSD-4-Clause",
    "CC01",
    "CC0-1.0",
    "CC0 1.0",
    "CDDL1",
    "CDDL-1.0",
    "Common Development and Distribution License 1.0",
    "EPL1",
    "EPL-1.0",
    "Eclipse Public License 1.0",
    "GPLv2",
    "GPL-2.0-only",
    "GPLv3",
    "GPL-3.0-only",
    "ISC",
    "LGPL",
    "LGPL-3.0-only",
    "LGPL2_1",
    "LGPL-2.1-only",
    "MIT",
    "MPL1_1",
    "MPL-1.1",
    "Mozilla Public License 1.1",
    "MPL2",
    "MPL-2.0",
    "Mozilla Public License 2.0",
    "NewBSD",
    "BSD-3-Clause",
    "New BSD",
    "OFL",
    "OFL-1.1",
    "SIL OPEN FONT LICENSE Version 1.1",
    "Python",
    "PSF-2.0",
    "Python Software Foundation License",
    "Ruby",
    "SimplifiedBSD",
    "BSD-2-Clause",
    "Simplified BSD",
    "WTFPL",
    "0BSD",
    "BSD Zero Clause License",
    "Zlib",
    "zlib/libpng license",
];

pub(super) async fn resolve_license_from_dir(
    manifest_license: Option<String>,
    dir: &Path,
) -> io::Result<Option<String>> {
    let Some(license_path) = first_license_path(dir).await? else {
        return Ok(manifest_license);
    };
    let contents = tokio::fs::read(license_path).await?;
    Ok(Some(
        detect_license_from_text(&String::from_utf8_lossy(&contents))
            .unwrap_or_else(|| "Unknown".to_string()),
    ))
}

async fn first_license_path(dir: &Path) -> io::Result<Option<std::path::PathBuf>> {
    for name in LICENSE_FILES {
        let path = dir.join(name);
        match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn detect_license_from_text(contents: &str) -> Option<String> {
    static LICENSE_PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = LICENSE_PATTERN.get_or_init(|| {
        let alternatives =
            LICENSE_NAMES.iter().map(|name| regex::escape(name)).collect::<Vec<_>>().join("|");
        Regex::new(&format!(r"(?i)\b({alternatives})\b"))
            .expect("license names form a valid regular expression")
    });
    let mut seen = HashSet::new();
    let matches = pattern
        .find_iter(contents)
        .map(|matched| matched.as_str())
        .filter(|matched| seen.insert(*matched))
        .collect::<Vec<_>>();
    (!matches.is_empty()).then(|| matches.join(" OR "))
}

#[cfg(test)]
mod tests;
