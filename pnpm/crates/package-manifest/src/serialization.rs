use super::{
    DEFAULT_INDENT, InitOptions, NamedTempFile, PackageManifest, PackageManifestError, Path,
    PathBuf, Serialize, Value, Write, convert_engines_runtime_to_dependencies, fs, io,
};

/// pnpm's on-write manifest normalization: within each dependency field,
/// sort the entries by name, and drop the field entirely when it holds no
/// entries.
pub(super) fn normalize_dependency_fields(manifest: &mut Value) {
    let Some(manifest) = manifest.as_object_mut() else { return };
    for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        let is_empty_object = match manifest.get_mut(field) {
            Some(Value::Object(deps)) => {
                deps.sort_keys();
                deps.is_empty()
            }
            _ => continue,
        };
        if is_empty_object {
            manifest.remove(field);
        }
    }
}

/// The indentation unit of a JSON document: the leading whitespace of its
/// first indented line. Empty for a single-line (or unindented) document,
/// which then round-trips back to its compact form.
fn detect_indent(contents: &str) -> &str {
    contents
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            (!trimmed.is_empty() && trimmed.len() < line.len())
                .then(|| &line[..line.len() - trimmed.len()])
        })
        .unwrap_or("")
}

/// Serialize with the manifest's own indentation unit; an empty unit
/// produces a compact single-line document. At most the first 10
/// characters of the unit are used — the cap `JSON.stringify` applies to
/// its `space` argument, which pnpm writes manifests through — so a
/// pathologically indented source file can't amplify the output.
pub(super) fn serialize_with_indent(
    value: &Value,
    indent: &str,
) -> Result<String, serde_json::Error> {
    if indent.is_empty() {
        return serde_json::to_string(value);
    }
    let indent = match indent.char_indices().nth(10) {
        Some((cap, _)) => &indent[..cap],
        None => indent,
    };
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value.serialize(&mut serializer)?;
    Ok(String::from_utf8(out).expect("serde_json emits UTF-8"))
}

/// Read `<dir>/package.json` if it exists, returning `Ok(None)` when the file
/// is absent. Other IO errors and JSON parse errors propagate.
///
/// A missing file is the only case that maps to `Ok(None)`; malformed JSON
/// surfaces as a `BAD_PACKAGE_JSON` error and other IO errors propagate.
pub fn safe_read_package_json_from_dir(dir: &Path) -> Result<Option<Value>, PackageManifestError> {
    let path = dir.join("package.json");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(PackageManifestError::Io(err)),
    };
    parse_manifest(&text).map(Some).map_err(|source| PackageManifestError::Parse { path, source })
}

/// Parse the contents of a `package.json`.
///
/// A leading UTF-8 byte order mark is dropped before parsing: editors
/// and publishers do write manifests with one (npm ships such packages),
/// `serde_json` rejects it, and pnpm decodes every manifest through
/// `strip-bom`/`TextDecoder`, which drop it. Route every manifest parse
/// through here so both stacks accept the same files.
pub fn parse_manifest(contents: &str) -> serde_json::Result<Value> {
    serde_json::from_str(strip_utf8_bom(contents))
}

/// [`parse_manifest`] for manifest bytes that have not been decoded yet,
/// such as an entry read straight out of a tarball.
pub fn parse_manifest_bytes(bytes: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes))
}

fn strip_utf8_bom(contents: &str) -> &str {
    contents.strip_prefix('\u{feff}').unwrap_or(contents)
}

impl PackageManifest {
    pub(super) fn write_to_file(
        path: &Path,
        manifest: &Value,
    ) -> Result<String, PackageManifestError> {
        let contents = serialize_with_indent(manifest, DEFAULT_INDENT)?;
        fs::write(path, format!("{contents}\n"))?; // TODO: forbid overwriting existing files
        Ok(contents)
    }

    /// Write `contents` to `path` atomically: a sibling temp file is written
    /// and fsynced, then renamed over `path`. A crash or write error therefore
    /// never leaves a truncated or partial `package.json` behind, matching the
    /// `write-file-atomic` guarantee.
    pub(super) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
        let dir = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut tmp = NamedTempFile::new_in(dir)?;
        tmp.write_all(contents.as_bytes())?;
        tmp.as_file().sync_all()?;
        // A NamedTempFile is created 0o600; preserve the original file's mode
        // when overwriting an existing package.json (write-file-atomic does the
        // same) so the rename doesn't silently tighten its permissions.
        if let Ok(metadata) = fs::metadata(path) {
            tmp.as_file().set_permissions(metadata.permissions())?;
        }
        tmp.persist(path).map_err(|err| err.error)?;
        Ok(())
    }

    pub(super) fn read_from_file(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        let file_contents = fs::read_to_string(&path)?;
        let contents = strip_utf8_bom(&file_contents);
        let mut value: Value = parse_manifest(contents)
            .map_err(|source| PackageManifestError::Parse { path: path.clone(), source })?;
        let mut on_disk = value.clone();
        normalize_dependency_fields(&mut on_disk);
        convert_engines_runtime_to_dependencies(&mut value, "devEngines", "devDependencies");
        convert_engines_runtime_to_dependencies(&mut value, "engines", "dependencies");
        Ok(PackageManifest {
            path,
            value,
            insert_final_newline: contents.ends_with('\n'),
            indent: detect_indent(contents).to_string(),
            on_disk: Some(on_disk),
        })
    }

    pub fn from_path(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        // The read itself answers existence: a NotFound maps to the
        // same missing-manifest error a pre-check would raise, without
        // paying a stat before every successful read.
        let rendered_path = path.display().to_string();
        PackageManifest::read_from_file(path).map_err(|error| match error {
            PackageManifestError::Io(io_error) if io_error.kind() == io::ErrorKind::NotFound => {
                PackageManifestError::NoImporterManifestFound(rendered_path)
            }
            other => other,
        })
    }

    pub fn create_if_needed(path: PathBuf) -> Result<PackageManifest, PackageManifestError> {
        if !path.exists() {
            let scaffold = PackageManifest::init_value_for(&path, InitOptions::default());
            PackageManifest::write_to_file(&path, &scaffold)?;
        }
        // Read the scaffold back rather than assembling the manifest by
        // hand, so its formatting and no-op-save baseline are derived from
        // the file the same way as for a pre-existing manifest.
        PackageManifest::read_from_file(path)
    }
}
