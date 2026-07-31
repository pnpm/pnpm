use regex::Regex;
use std::{collections::HashSet, path::Path, sync::OnceLock};
use tokio::io::AsyncReadExt;

const MAX_LICENSE_FILE_SIZE: usize = 1024 * 1024;

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
) -> Option<String> {
    let Some(contents) = read_first_license_file(dir).await else {
        return manifest_license;
    };
    Some(
        detect_license_from_text(&String::from_utf8_lossy(&contents))
            .unwrap_or_else(|| "Unknown".to_string()),
    )
}

async fn read_first_license_file(dir: &Path) -> Option<Vec<u8>> {
    for name in LICENSE_FILES {
        let path = dir.join(name);
        let Ok(file) = open_no_follow(&path).await else { continue };
        let metadata = match file.metadata().await {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) | Err(_) => continue,
        };
        if metadata.len() > MAX_LICENSE_FILE_SIZE as u64 {
            continue;
        }
        let mut contents = Vec::with_capacity(metadata.len() as usize);
        if file.take((MAX_LICENSE_FILE_SIZE + 1) as u64).read_to_end(&mut contents).await.is_err() {
            continue;
        }
        if contents.len() > MAX_LICENSE_FILE_SIZE {
            continue;
        }
        return Some(contents);
    }
    None
}

async fn open_no_follow(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    #[cfg(windows)]
    options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path).await
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
