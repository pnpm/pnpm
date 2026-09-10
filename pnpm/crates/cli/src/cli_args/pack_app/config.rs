use super::{
    Component, MIN_BUILDER_VERSION, PackAppError, Path, SUPPORTED_OS, Value, fs, parse_manifest,
};

/// A parsed `<os>-<arch>[-<libc>]` target triplet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedTarget {
    pub(super) raw: String,
    pub(super) platform: String,
    pub(super) arch: String,
    pub(super) libc: Option<String>,
}

/// The Node.js version pack-app embeds when neither `--runtime` nor
/// `pnpm.app.runtime` is set.
///
/// pnpm defaults to its own running interpreter's version
/// (`process.version`). pacquet has no embedded Node.js, so it falls back
/// to the minimum SEA-capable version, which the embedded-runtime check
/// then accepts.
pub(super) fn default_runtime_version() -> String {
    format!("{}.{}.0", MIN_BUILDER_VERSION.0, MIN_BUILDER_VERSION.1)
}

/// Whether a repo-controlled `entry` / `outputDir` value could escape the
/// project directory once joined onto it: an absolute path, a `..` traversal
/// component, or a Windows root-relative (`\foo`) / drive-relative (`C:foo`)
/// form. `Path::is_absolute` returns `false` for the latter two on Windows,
/// so they are matched explicitly via `RootDir` / `Prefix`.
pub(super) fn escapes_project(raw: &str) -> bool {
    let path = Path::new(raw);
    path.is_absolute()
        || path.components().any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
}

/// Whether `path` resolves (symlinks included) to a location inside `base`.
/// Both are canonicalized so a same-name symlink that points out of the
/// project is caught even though the lexical join looked contained. A
/// canonicalization failure is treated as "not within" (fail closed).
pub(super) fn path_is_within(path: &Path, base: &Path) -> bool {
    let (Ok(canonical_path), Ok(canonical_base)) =
        (dunce::canonicalize(path), dunce::canonicalize(base))
    else {
        return false;
    };
    canonical_path.starts_with(&canonical_base)
}

/// The on-disk file name of the produced executable for a target: a bare
/// name on POSIX, suffixed with `.exe` on Windows.
pub(super) fn output_file_name(output_name: &str, platform: &str) -> String {
    if platform == "win32" { format!("{output_name}.exe") } else { output_name.to_string() }
}

pub(super) fn parse_target(raw: &str) -> Result<ParsedTarget, PackAppError> {
    // Anchored, segment-constrained parse so inputs like
    // `linux-x64-musl-../../outside` are rejected outright — otherwise
    // `target.raw` would later flow into the output directory join and
    // could escape it.
    let parts: Vec<&str> = raw.split('-').collect();
    let invalid = || PackAppError::InvalidTarget {
        raw: raw.to_string(),
        supported_os: SUPPORTED_OS.join("|"),
    };
    let (platform, arch, libc) = match parts.as_slice() {
        [platform, arch] => (*platform, *arch, None),
        [platform, arch, libc] => (*platform, *arch, Some(*libc)),
        _ => return Err(invalid()),
    };
    if !SUPPORTED_OS.contains(&platform) {
        return Err(invalid());
    }
    if arch != "x64" && arch != "arm64" {
        return Err(invalid());
    }
    if let Some(libc) = libc {
        if libc != "musl" {
            return Err(invalid());
        }
        if platform != "linux" {
            return Err(PackAppError::MuslOnNonLinux { raw: raw.to_string() });
        }
    }
    Ok(ParsedTarget {
        raw: raw.to_string(),
        platform: platform.to_string(),
        arch: arch.to_string(),
        libc: libc.map(ToString::to_string),
    })
}

/// Runtime spec is `<name>@<version>`. Only `node` is supported today; the
/// prefix is kept so future runtimes (bun, deno) can share the flag
/// without a breaking change.
pub(super) fn parse_runtime(spec: &str) -> Result<String, PackAppError> {
    let invalid = || PackAppError::InvalidRuntime { spec: spec.to_string() };
    let (name, version) = spec.split_once('@').ok_or_else(invalid)?;
    if name != "node" || version.is_empty() {
        return Err(invalid());
    }
    Ok(version.to_string())
}

/// Win32 reserved device names (case-insensitive, with or without an
/// extension).
pub(super) fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

/// Reject anything that would let the output escape its target directory,
/// or that would fail filesystem-level validation on any supported host.
pub(super) fn validate_output_name(name: &str) -> Result<String, PackAppError> {
    let basename = Path::new(name).file_name().and_then(|n| n.to_str());
    let invalid_chars =
        name.chars().any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\0'));
    let trailing_dot_or_space = name.ends_with('.') || name.ends_with(' ');
    if basename != Some(name)
        || name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || invalid_chars
        || is_reserved_windows_name(name)
        || trailing_dot_or_space
    {
        return Err(PackAppError::InvalidOutputName { name: name.to_string() });
    }
    Ok(name.to_string())
}

/// Fields pack-app reads from `pnpm.app` in package.json.
#[derive(Debug, Default)]
pub(super) struct ProjectAppConfig {
    pub(super) entry: Option<String>,
    pub(super) targets: Vec<String>,
    pub(super) runtime: Option<String>,
    pub(super) output_dir: Option<String>,
    pub(super) output_name: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct ReadProjectAppConfigResult {
    pub(super) name: Option<String>,
    pub(super) app: Option<ProjectAppConfig>,
}

/// A narrow reader just for this command: a `package.json` with optional
/// `pnpm.app` settings, without the installable/engine checks the regular
/// manifest reader would impose.
pub(super) fn read_project_app_config(
    dir: &Path,
) -> Result<ReadProjectAppConfigResult, PackAppError> {
    let manifest_path = dir.join("package.json");
    let Ok(raw) = fs::read_to_string(&manifest_path) else {
        return Ok(ReadProjectAppConfigResult::default());
    };
    let manifest: Value = parse_manifest(&raw).map_err(|err| PackAppError::InvalidPackageJson {
        path: manifest_path.display().to_string(),
        message: err.to_string(),
    })?;
    let Some(manifest) = manifest.as_object() else {
        return Ok(ReadProjectAppConfigResult::default());
    };
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let app_field = manifest
        .get("pnpm")
        .and_then(Value::as_object)
        .and_then(|pnpm| pnpm.get("app"))
        .and_then(Value::as_object);
    let Some(app_field) = app_field else {
        return Ok(ReadProjectAppConfigResult { name, app: None });
    };
    Ok(ReadProjectAppConfigResult { name, app: Some(validate_app_config(app_field)?) })
}

fn validate_app_config(
    raw: &serde_json::Map<String, Value>,
) -> Result<ProjectAppConfig, PackAppError> {
    const KNOWN: &[&str] = &["entry", "targets", "runtime", "outputDir", "outputName"];
    for key in raw.keys() {
        if !KNOWN.contains(&key.as_str()) {
            return Err(PackAppError::UnknownConfigKey {
                key: key.clone(),
                allowed: KNOWN.join(", "),
            });
        }
    }
    let string_field = |key: &str| -> Result<Option<String>, PackAppError> {
        match raw.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(PackAppError::InvalidConfig {
                message: format!(r#""pnpm.app.{key}" must be a string."#),
            }),
        }
    };
    let targets = app_targets(raw)?;
    Ok(ProjectAppConfig {
        entry: string_field("entry")?,
        targets,
        runtime: string_field("runtime")?,
        output_dir: string_field("outputDir")?,
        output_name: string_field("outputName")?,
    })
}

pub(super) fn derive_output_name_from_package(
    project: &ReadProjectAppConfigResult,
    dir: &Path,
) -> Result<String, PackAppError> {
    let Some(name) = project.name.as_deref() else {
        return Err(PackAppError::NoOutputName { dir: dir.display().to_string() });
    };
    // Strip the `@scope/` prefix from scoped packages so the binary name is
    // a plain filename. The downstream `validate_output_name` pass rejects
    // any leftover path separators.
    let unscoped = if let Some(rest) = name.strip_prefix('@') {
        rest.split_once('/').map_or(name, |(_, rest)| rest)
    } else {
        name
    };
    Ok(unscoped.to_string())
}

fn app_targets(raw: &serde_json::Map<String, Value>) -> Result<Vec<String>, PackAppError> {
    Ok(match raw.get("targets") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(ToString::to_string).ok_or_else(|| PackAppError::InvalidConfig {
                    message: r#""pnpm.app.targets" must be an array of strings."#.to_string(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(PackAppError::InvalidConfig {
                message: r#""pnpm.app.targets" must be an array of strings."#.to_string(),
            });
        }
    })
}
