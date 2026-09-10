use super::{
    BundleDependencies, DependencyGroup, InitAuthor, InitOptions, NamedTempFile, PackageManifest,
    PackageManifestError, Pipe, Write, assert_eq, assert_snapshot, extract_license, json,
    parse_manifest_bytes, read_to_string, safe_read_package_json_from_dir, tempdir,
};

#[cfg(unix)]
#[test]
fn save_preserves_the_existing_package_json_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, r#"{"name":"perm","version":"1.0.0"}"#).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    // The atomic temp-file-then-rename must keep the original mode, not leave
    // the NamedTempFile's default 0o600 behind.
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

#[test]
fn test_init_package_json_content() {
    let manifest = PackageManifest::create_init_package_json("test", InitOptions::default());
    assert_snapshot!(serde_json::to_string_pretty(&manifest).unwrap());
}

#[test]
fn init_package_json_content_with_every_init_option() {
    let manifest = PackageManifest::create_init_package_json(
        "test",
        InitOptions {
            es_module: true,
            pinned_pnpm_version: Some("11.22.0"),
            author: InitAuthor {
                name: Some("pnpm"),
                email: Some("xxxxxx@pnpm.com"),
                url: Some("https://www.github.com/pnpm"),
            },
            license: Some("MIT"),
            version: Some("2.0.0"),
        },
    );
    assert_snapshot!(serde_json::to_string_pretty(&manifest).unwrap());
}

#[test]
fn init_should_create_package_json_if_not_exist() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    PackageManifest::init(&tmp, InitOptions::default()).unwrap();
    eprintln!("tmp={tmp:?} exists={} is_file={}", tmp.exists(), tmp.is_file());
    assert!(tmp.exists());
    assert!(tmp.is_file());
    assert_eq!(PackageManifest::from_path(tmp.clone()).unwrap().path, tmp);
}

#[test]
fn bundle_dependencies() {
    fn bundle_list<List>(list: List) -> BundleDependencies
    where
        List: IntoIterator,
        List::Item: Into<String>,
    {
        list.into_iter().map(Into::into).collect::<Vec<_>>().pipe(BundleDependencies::List)
    }

    macro_rules! case {
        ($input:expr => $output:expr) => {{
            let data = $input;
            eprintln!("CASE: {data}");
            let tmp = NamedTempFile::new().unwrap();
            write!(tmp.as_file(), "{}", data).unwrap();
            let manifest = PackageManifest::create_if_needed(tmp.path().to_path_buf()).unwrap();
            let bundle = manifest.bundle_dependencies().unwrap();
            assert_eq!(bundle, $output);
        }};
    }

    case!(r#"{ "bundleDependencies": ["foo", "bar"] }"# => Some(bundle_list(["foo", "bar"])));
    case!(r#"{ "bundledDependencies": ["foo", "bar"] }"# => Some(bundle_list(["foo", "bar"])));
    case!(r#"{ "bundleDependencies": false }"# => false.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundledDependencies": false }"# => false.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundleDependencies": true }"# => true.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundledDependencies": true }"# => true.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r"{}" => None);
}

#[test]
fn save_and_get_written_value_returns_saved_manifest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(path.clone()).unwrap();
    manifest.add_dependency("node", "runtime:22", DependencyGroup::Dev).unwrap();

    let written = manifest.save_and_get_written_value().unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&read_to_string(path).unwrap()).expect("parse saved manifest");

    assert_eq!(written, saved);
    assert_eq!(
        saved.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        })),
    );
    // The reification-only `devDependencies` ends up empty after the fold
    // and is dropped from the written file, while the in-memory manifest
    // keeps the reified entry.
    assert_eq!(saved.get("devDependencies"), None);
    assert_eq!(manifest.value().get("devDependencies"), Some(&json!({ "node": "runtime:22" })));
}

/// A manifest scaffolded for a project with no `package.json` ends with a
/// newline, both as created and after a save.
#[test]
fn new_manifests_end_with_a_final_newline() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(tmp.clone()).unwrap();
    assert!(read_to_string(&tmp).unwrap().ends_with('\n'));
    manifest.save().unwrap();
    assert!(read_to_string(&tmp).unwrap().ends_with('\n'));
}

/// Editors on Windows — and published packages such as Vite's
/// `utf8-bom-package` fixture — write manifests with a leading UTF-8 BOM.
/// pnpm decodes them through `strip-bom`, so pacquet must accept them too
/// instead of failing at line 1 column 1.
#[test]
fn from_path_reads_a_manifest_that_starts_with_a_utf8_bom() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, "\u{feff}{\n  \"name\": \"fixture\",\n  \"version\": \"1.0.0\"\n}\n")
        .unwrap();

    let manifest = PackageManifest::from_path(path).unwrap();
    assert_eq!(manifest.value().get("name").unwrap(), &json!("fixture"));
    assert_eq!(manifest.value().get("version").unwrap(), &json!("1.0.0"));
}

/// The BOM belongs to the file, not to the manifest, so reading one never
/// rewrites the file on its own; the BOM only goes away when a real change
/// makes the writer emit the manifest afresh, matching what pnpm writes.
#[test]
fn saving_a_bom_prefixed_manifest_keeps_the_bom_until_something_changes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = "\u{feff}{\n  \"name\": \"fixture\",\n  \"version\": \"1.0.0\"\n}\n";
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);

    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    let saved = read_to_string(&path).unwrap();
    eprintln!("SAVED:\n{saved}");
    assert!(!saved.starts_with('\u{feff}'));
    assert!(saved.contains(r#""fastify": "1.0.0""#));
}

#[test]
fn safe_read_package_json_from_dir_reads_a_manifest_that_starts_with_a_utf8_bom() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("package.json"), "\u{feff}{\"name\":\"fixture\"}").unwrap();

    let manifest = safe_read_package_json_from_dir(dir.path()).unwrap().unwrap();
    assert_eq!(manifest.get("name").unwrap(), &json!("fixture"));
}

#[test]
fn extracts_license_from_modern_and_legacy_manifest_fields() {
    assert_eq!(extract_license(&json!({ "license": "MIT" })), Some("MIT".to_string()));
    assert_eq!(
        extract_license(&json!({ "license": { "type": "Apache-2.0" } })),
        Some("Apache-2.0".to_string()),
    );
    assert_eq!(
        extract_license(&json!({ "licenses": [{ "type": "MIT" }] })),
        Some("MIT".to_string()),
    );
    assert_eq!(
        extract_license(&json!({
            "licenses": [{ "type": "MIT" }, { "name": "Apache-2.0" }]
        })),
        Some("(MIT OR Apache-2.0)".to_string()),
    );
}

#[test]
fn modern_license_takes_priority_and_invalid_values_fall_back() {
    assert_eq!(
        extract_license(&json!({
            "license": "BSD-3-Clause",
            "licenses": [{ "type": "MIT" }]
        })),
        Some("BSD-3-Clause".to_string()),
    );
    assert_eq!(
        extract_license(&json!({
            "license": "",
            "licenses": [{ "type": "MIT" }]
        })),
        Some("MIT".to_string()),
    );
    assert_eq!(extract_license(&json!({ "license": 42, "licenses": [] })), None);
}

/// A BOM is only stripped where a document may legitimately start, so a
/// stray one in the middle of the manifest stays a parse error.
#[test]
fn a_bom_after_the_start_of_the_manifest_is_still_a_parse_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, "{\u{feff}\"name\":\"fixture\"}").unwrap();

    let Err(err) = PackageManifest::from_path(path.clone()) else {
        panic!("a stray BOM must not parse")
    };
    eprintln!("ERR: {err}");
    assert!(
        matches!(&err, PackageManifestError::Parse { path: reported, .. } if reported == &path),
        "the offending manifest path must be reported, got {err:?}",
    );
    assert!(err.to_string().contains(&path.display().to_string()));
}

#[test]
fn parse_manifest_bytes_accepts_undecoded_bom_prefixed_bytes() {
    let manifest = parse_manifest_bytes(b"\xEF\xBB\xBF{\"name\":\"fixture\"}").unwrap();
    assert_eq!(manifest.get("name").unwrap(), &json!("fixture"));
}
