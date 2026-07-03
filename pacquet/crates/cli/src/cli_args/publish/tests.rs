use super::PublishArgs;
use pacquet_config::Config;
use pacquet_publish::Access;
use pretty_assertions::assert_eq;

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
    let mut config = Config::default();
    config.ignore_scripts = false;
    assert!(!publish_args().should_ignore_scripts(&config));
    assert!(PublishArgs { ignore_scripts: true, ..publish_args() }.should_ignore_scripts(&config));
    config.ignore_scripts = true;
    assert!(publish_args().should_ignore_scripts(&config));
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
