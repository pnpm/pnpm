use super::{PublishArgs, run_publish_scripts};
use pacquet_config::Config;
use pacquet_publish::Access;
use pacquet_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use serde_json::json;

/// A `PublishArgs` with every flag at its default; a test overrides only the
/// field it exercises.
fn publish_args() -> PublishArgs {
    PublishArgs {
        package: None,
        dry_run: false,
        json: false,
        tag: None,
        access: None,
        provenance: false,
        ignore_scripts: false,
        skip_manifest_obfuscation: false,
        otp: None,
        publish_branch: None,
        no_git_checks: false,
        force: false,
        batch: false,
        report_summary: false,
    }
}

#[test]
fn should_ignore_scripts_ors_the_flag_with_the_config() {
    let config_off = Config { ignore_scripts: false, ..Default::default() };
    let config_on = Config { ignore_scripts: true, ..Default::default() };
    assert!(!publish_args().should_ignore_scripts(&config_off));
    assert!(
        PublishArgs { ignore_scripts: true, ..publish_args() }.should_ignore_scripts(&config_off),
    );
    assert!(publish_args().should_ignore_scripts(&config_on));
}

#[test]
fn publish_options_defaults_the_tag_to_latest_and_carries_the_otp() {
    let options = publish_args().publish_options(&Config::default(), Some("246810".to_owned()));
    assert_eq!(options.tag, "latest");
    assert_eq!(options.otp, Some("246810".to_owned()));
    assert_eq!(options.provenance, None);
    assert_eq!(options.access, None);
    assert!(!options.dry_run);
    assert!(!options.stage);
}

#[test]
fn publish_options_applies_tag_access_provenance_and_dry_run() {
    let args = PublishArgs {
        tag: Some("next".to_owned()),
        access: Some("restricted".to_owned()),
        provenance: true,
        dry_run: true,
        ..publish_args()
    };
    let options = args.publish_options(&Config::default(), None);
    assert_eq!(options.tag, "next");
    assert_eq!(options.access, Some(Access::Restricted));
    assert_eq!(options.provenance, Some(true));
    assert!(options.dry_run);
    assert_eq!(options.otp, None);
}

#[test]
fn pack_for_publish_writes_a_tarball_and_returns_the_manifest() {
    let dir = tempfile::tempdir().expect("a source dir");
    std::fs::write(dir.path().join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write the manifest");
    let dest = tempfile::tempdir().expect("a destination dir");

    let args = PublishArgs { ignore_scripts: true, ..publish_args() };
    let result = args
        .pack_for_publish::<SilentReporter>(dir.path(), &Config::default(), dest.path())
        .expect("packing succeeds");

    assert_eq!(result.published_manifest["name"], "pkg");
    let wrote_tarball = std::fs::read_dir(dest.path())
        .expect("read the destination")
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "tgz"));
    assert!(wrote_tarball, "a .tgz should be written to the destination");
}

/// The publish-lifecycle scripts run through `sh -c` in the package
/// directory, so a `prepublishOnly` that writes a file leaves it in `dir`.
#[cfg(unix)]
#[test]
fn run_publish_scripts_runs_the_declared_lifecycle_scripts() {
    let dir = tempfile::tempdir().expect("a package dir");
    let manifest = json!({ "name": "pkg", "version": "1.0.0", "scripts": { "prepublishOnly": "echo ok > ran.txt" } });

    run_publish_scripts::<SilentReporter>(
        dir.path(),
        &Config::default(),
        &manifest,
        &["prepublishOnly"],
    )
    .expect("the script runs");

    let marker =
        std::fs::read_to_string(dir.path().join("ran.txt")).expect("the marker is written");
    assert_eq!(marker.trim(), "ok");
}

#[test]
fn run_publish_scripts_is_a_noop_when_no_script_is_declared() {
    let dir = tempfile::tempdir().expect("a package dir");
    let manifest = json!({ "name": "pkg", "version": "1.0.0" });

    run_publish_scripts::<SilentReporter>(
        dir.path(),
        &Config::default(),
        &manifest,
        &["prepublishOnly", "publish", "postpublish"],
    )
    .expect("a no-op succeeds");

    assert!(!dir.path().join("ran.txt").exists());
}
