use crate::{
    DependencyGroup, ManifestFormat, PackageManifest, PackageManifestError,
    safe_read_project_manifest_from_dir,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn project_manifests_reject_non_object_roots() {
    for filename in ["package.json", "package.json5", "package.yaml"] {
        for source in ["[]", "42", "true", r#""fixture""#] {
            let dir = tempdir().unwrap();
            let path = dir.path().join(filename);
            fs::write(&path, source).unwrap();
            let error =
                PackageManifest::from_path(path.clone()).err().expect("reject non-object root");
            eprintln!("ERROR: {error}");
            let expected_code = if filename == "package.yaml" {
                "ERR_PNPM_PACKAGE_MANIFEST_INVALID_ATTRIBUTE"
            } else {
                "ERR_PNPM_INVALID_MANIFEST"
            };
            assert_eq!(miette::Diagnostic::code(&error).unwrap().to_string(), expected_code);
            let raw_error =
                safe_read_project_manifest_from_dir(dir.path(), ManifestFormat::default())
                    .unwrap_err();
            assert_eq!(miette::Diagnostic::code(&raw_error).unwrap().to_string(), expected_code);
            assert!(error.to_string().contains("the manifest root must be an object"));
            assert!(
                error
                    .to_string()
                    .contains(&path.display().to_string()),
            );
            assert_eq!(fs::read_to_string(path).unwrap(), source);
        }
    }
}

#[test]
fn json_null_roots_keep_the_invalid_manifest_error_code() {
    for filename in ["package.json", "package.json5"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(filename);
        fs::write(&path, "null").unwrap();
        let error = PackageManifest::from_path(path.clone()).err().unwrap();
        eprintln!("ERROR: {error}");
        assert_eq!(
            miette::Diagnostic::code(&error).unwrap().to_string(),
            "ERR_PNPM_INVALID_MANIFEST",
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "null");
    }
}

#[test]
fn json5_reads_bom_comments_and_json5_syntax() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json5");
    fs::write(&path, "\u{feff}// project\n{ name: 'fixture', version: '1.0.0', custom: -0x10, }\n")
        .unwrap();
    let manifest = PackageManifest::from_path(path).unwrap();
    assert_eq!(manifest.value()["name"], "fixture");
    assert_eq!(manifest.value()["version"], "1.0.0");
    assert_eq!(manifest.value()["custom"], -16);
}

#[test]
fn json5_noop_save_preserves_original_bytes() {
    for newline in ["\n", "\r\n"] {
        for final_newline in [false, true] {
            let dir = tempdir().unwrap();
            let path = dir.path().join("package.json5");
            let mut source = "\u{feff}// project\n{\n\tname: 'fixture',\n\tversion: '1.0.0',\n}"
                .replace('\n', newline);
            if final_newline {
                source.push_str(newline);
            }
            fs::write(&path, &source).unwrap();
            let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
            manifest.save().unwrap();
            let written = fs::read_to_string(&path).unwrap();
            eprintln!("WRITTEN:\n{written}");
            assert_eq!(written, source);
        }
    }
}

#[test]
fn json5_changed_save_preserves_crlf_and_final_newline_choice() {
    for final_newline in [false, true] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json5");
        let mut source = "{\r\n  name: 'fixture',\r\n  version: '1.0.0',\r\n}".to_owned();
        if final_newline {
            source.push_str("\r\n");
        }
        fs::write(&path, source).unwrap();
        let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
        manifest.value_mut()["version"] = json!("2.0.0");
        manifest.save().unwrap();
        let written = fs::read_to_string(&path).unwrap();
        eprintln!("WRITTEN:\n{written}");
        assert!(written.contains("\r\n"));
        assert!(!written.replace("\r\n", "").contains('\n'));
        assert_eq!(written.ends_with("\r\n"), final_newline);
        assert_eq!(PackageManifest::from_path(path).unwrap().value()["version"], "2.0.0");
    }
}

#[test]
fn json5_save_preserves_comments_when_version_and_dependencies_change() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json5");
    let source = "// project\n{\n  name: 'fixture',\n  version: '1.0.0', // version note\n  // dependencies note\n  dependencies: {\n    alpha: '1.0.0', // alpha note\n    obsolete: '1.0.0',\n  },\n  custom: { url: 'https://example.test/*literal*/' },\n}\n";
    fs::write(&path, source).unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.value_mut()["version"] = json!("2.0.0");
    manifest.add_dependency("alpha", "2.0.0", DependencyGroup::Prod).unwrap();
    manifest.add_dependency("bravo", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.value_mut()["dependencies"]
        .as_object_mut()
        .unwrap()
        .remove("obsolete");
    manifest.save().unwrap();
    let written = fs::read_to_string(&path).unwrap();
    eprintln!("WRITTEN:\n{written}");
    for comment in ["// project", "// version note", "// dependencies note", "// alpha note"] {
        assert!(written.contains(comment), "missing {comment}");
    }
    let reread = PackageManifest::from_path(path.clone()).unwrap();
    dbg!(reread.value());
    assert_eq!(reread.value()["version"], "2.0.0");
    assert_eq!(reread.value()["dependencies"], json!({"alpha": "2.0.0", "bravo": "1.0.0"}));
    assert_eq!(reread.value()["custom"]["url"], "https://example.test/*literal*/");
    assert!(!dir.path().join("package.json").exists());
    manifest.save().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), written);
}

#[test]
fn json_manifest_still_rejects_json5_syntax() {
    for source in ["{name: 'fixture'}", r#"{"name": "fixture",}"#, "{/* comment */}"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.json");
        fs::write(&path, source).unwrap();
        let error = PackageManifest::from_path(path.clone()).err().unwrap();
        eprintln!("ERROR: {error}");
        assert!(
            error
                .to_string()
                .contains(&path.display().to_string()),
        );
        assert_eq!(fs::read_to_string(path).unwrap(), source);
    }
}

#[test]
fn json5_save_recreates_a_removed_manifest_with_updated_values() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json5");
    fs::write(&path, "{name: 'fixture', version: '1.0.0'}").unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    fs::remove_file(&path).unwrap();
    manifest.value_mut()["version"] = json!("2.0.0");
    manifest.save().unwrap();
    let reread = PackageManifest::from_path(path).unwrap();
    dbg!(reread.value());
    assert_eq!(reread.value(), manifest.value());
    assert_eq!(reread.value()["version"], "2.0.0");
}

#[test]
fn json5_save_reports_read_errors_without_overwriting_the_source() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json5");
    fs::write(&path, "{name: 'fixture'}").unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    let invalid_utf8 = [0xff];
    fs::write(&path, invalid_utf8).unwrap();
    manifest.value_mut()["version"] = json!("2.0.0");
    let error = manifest.save().unwrap_err();
    dbg!(&error);
    assert!(matches!(error, PackageManifestError::Read { path: error_path, source }
        if error_path == path && source.kind() == std::io::ErrorKind::InvalidData));
    assert_eq!(fs::read(path).unwrap(), invalid_utf8);
}

#[test]
fn unreadable_preferred_manifest_does_not_fall_back_to_another_format() {
    for preferred in ["package.json", "package.json5"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(preferred);
        fs::write(&path, [0xff]).unwrap();
        fs::write(dir.path().join("package.yaml"), "name: fallback\n").unwrap();
        let error =
            safe_read_project_manifest_from_dir(dir.path(), ManifestFormat::default()).unwrap_err();
        dbg!(&error);
        assert!(matches!(error, PackageManifestError::Read { path: error_path, source }
            if error_path == path && source.kind() == std::io::ErrorKind::InvalidData));
        assert_eq!(fs::read(path).unwrap(), [0xff]);
    }
}
