use super::{PublishArgs, PublishFlags, run_publish_scripts};
use crate::cli_args::{CliArgs, cli_command::CliCommand};
use clap::Parser;
use pnpm_config::Config;
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_publish::PublishNetwork;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;
use serde_json::json;

/// A `PublishArgs` with every flag at its default; a test overrides only the
/// field it exercises.
fn publish_args() -> PublishArgs {
    PublishArgs { package: None, flags: publish_flags() }
}

fn publish_args_with(flags: PublishFlags) -> PublishArgs {
    PublishArgs { package: None, flags }
}

fn parsed_publish_flags(argv: &[&str]) -> PublishFlags {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Publish(publish) => publish.flags,
        other => panic!("expected publish, got {other:?}"),
    }
}

fn publish_flags() -> PublishFlags {
    PublishFlags {
        dry_run: false,
        json: false,
        tag: None,
        access: None,
        provenance: false,
        no_provenance: false,
        ignore_scripts: false,
        embed_readme: false,
        no_embed_readme: false,
        skip_manifest_obfuscation: false,
        no_skip_manifest_obfuscation: false,
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
        publish_args_with(PublishFlags { ignore_scripts: true, ..publish_flags() })
            .should_ignore_scripts(&config_off),
    );
    assert!(publish_args().should_ignore_scripts(&config_on));
}

#[test]
fn publish_options_defaults_the_tag_to_latest_and_carries_the_otp() {
    let options =
        publish_args().publish_options(&Config::default(), Some("246810".to_owned()), false);
    assert_eq!(options.tag, "latest");
    assert_eq!(options.otp, Some("246810".to_owned()));
    assert_eq!(options.provenance, None);
    assert_eq!(options.access, None);
    assert!(!options.dry_run);
    assert!(!options.stage);
}

#[test]
fn publish_options_applies_tag_access_provenance_and_dry_run() {
    let args = publish_args_with(PublishFlags {
        tag: Some("next".to_owned()),
        access: Some("restricted".to_owned()),
        provenance: true,
        dry_run: true,
        ..publish_flags()
    });
    let options = args.publish_options(&Config::default(), None, false);
    assert_eq!(options.tag, "next");
    assert_eq!(options.access.as_deref(), Some("restricted"));
    assert_eq!(options.provenance, Some(true));
    assert!(options.dry_run);
    assert_eq!(options.otp, None);
}

#[test]
fn publish_options_falls_back_to_the_configured_settings() {
    let config = Config {
        access: Some("restricted".to_owned()),
        tag: Some("next".to_owned()),
        provenance: Some(true),
        ..Default::default()
    };
    let options = publish_args().publish_options(&config, None, false);
    assert_eq!(options.access.as_deref(), Some("restricted"));
    assert_eq!(options.tag, "next");
    assert_eq!(options.provenance, Some(true));
}

#[test]
fn publish_flags_outrank_the_configured_settings() {
    let config = Config {
        access: Some("restricted".to_owned()),
        tag: Some("next".to_owned()),
        ..Default::default()
    };
    let args = publish_args_with(PublishFlags {
        tag: Some("from-flag".to_owned()),
        access: Some("public".to_owned()),
        ..publish_flags()
    });
    let options = args.publish_options(&config, None, false);
    assert_eq!(options.access.as_deref(), Some("public"));
    assert_eq!(options.tag, "from-flag");
}

/// A configured `provenance: false` has to reach the publish options as an
/// explicit `false` — that is what suppresses the attestation the OIDC
/// exchange would otherwise turn on.
#[test]
fn configured_provenance_false_reaches_the_publish_options() {
    let config = Config { provenance: Some(false), ..Default::default() };
    assert_eq!(publish_args().publish_options(&config, None, false).provenance, Some(false));
}

#[test]
fn provenance_flag_outranks_a_configured_false() {
    let config = Config { provenance: Some(false), ..Default::default() };
    let args = publish_args_with(PublishFlags { provenance: true, ..publish_flags() });
    assert_eq!(args.publish_options(&config, None, false).provenance, Some(true));
}

#[test]
fn provenance_pair_resolves_last_one_wins() {
    assert!(parsed_publish_flags(&["pacquet", "publish", "--provenance"]).provenance);
    assert!(parsed_publish_flags(&["pacquet", "publish", "--no-provenance"]).no_provenance);

    // Both spellings in one argv must not error (pnpm forwards raw tokens);
    // mutual `overrides_with` collapses them to the last-specified.
    let last_off = parsed_publish_flags(&["pacquet", "publish", "--provenance", "--no-provenance"]);
    assert!(last_off.no_provenance && !last_off.provenance, "--no-provenance wins when last");
    let last_on = parsed_publish_flags(&["pacquet", "publish", "--no-provenance", "--provenance"]);
    assert!(last_on.provenance && !last_on.no_provenance, "--provenance wins when last");
}

#[test]
fn no_provenance_flag_outranks_a_configured_true() {
    let config = Config { provenance: Some(true), ..Default::default() };
    let args = publish_args_with(PublishFlags { no_provenance: true, ..publish_flags() });
    assert_eq!(args.publish_options(&config, None, false).provenance, Some(false));
}

/// See [`pnpm_publish::resolve_access`] for why an unrecognized value is
/// not dropped.
#[test]
fn an_unrecognized_configured_access_is_kept_verbatim() {
    let config = Config { access: Some("everyone".to_owned()), ..Default::default() };
    assert_eq!(
        publish_args().publish_options(&config, None, false).access.as_deref(),
        Some("everyone"),
    );
}

/// `""` resolves to `None`, but whitespace does not: upstream's
/// `opts.publishBranch ? … : ['master','main']` is a truthiness test, and
/// `"  "` is truthy there, so the git checks reject every real branch.
#[test]
fn resolved_publish_branch_treats_only_an_empty_value_as_unset() {
    let configured = Config { publish_branch: Some("release".to_owned()), ..Default::default() };
    assert_eq!(publish_flags().resolved_publish_branch(&configured), Some("release"));
    assert_eq!(publish_flags().resolved_publish_branch(&Config::default()), None);

    let flagged = PublishFlags { publish_branch: Some("from-flag".to_owned()), ..publish_flags() };
    assert_eq!(flagged.resolved_publish_branch(&configured), Some("from-flag"));

    let empty = Config { publish_branch: Some(String::new()), ..Default::default() };
    assert_eq!(publish_flags().resolved_publish_branch(&empty), None);
    let blank = Config { publish_branch: Some("  ".to_owned()), ..Default::default() };
    assert_eq!(publish_flags().resolved_publish_branch(&blank), Some("  "));
}

#[test]
fn resolved_otp_prefers_the_flag_then_the_config() {
    let config = Config { otp: Some("from-config".to_owned()), ..Default::default() };
    assert_eq!(publish_flags().resolved_otp(&config), Some("from-config".to_owned()));

    let flags = PublishFlags { otp: Some("from-flag".to_owned()), ..publish_flags() };
    assert_eq!(flags.resolved_otp(&config), Some("from-flag".to_owned()));
    assert_eq!(flags.resolved_otp(&Config::default()), Some("from-flag".to_owned()));
    assert_eq!(publish_flags().resolved_otp(&Config::default()), None);
}

#[tokio::test]
async fn pack_for_publish_writes_a_tarball_and_returns_the_manifest() {
    let dir = tempfile::tempdir().expect("a source dir");
    std::fs::write(dir.path().join("package.json"), r#"{"name":"pkg","version":"1.0.0"}"#)
        .expect("write the manifest");
    let dest = tempfile::tempdir().expect("a destination dir");

    let args = publish_args_with(PublishFlags { ignore_scripts: true, ..publish_flags() });
    let result = args
        .pack_for_publish::<SilentReporter>(dir.path(), &Config::default(), dest.path())
        .await
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

/// Publishing a directory that has no `package.json` fails with
/// `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND` before any packing or network work —
/// the only branch of `publish_directory` reachable without a live registry.
#[tokio::test]
async fn publish_directory_errors_when_no_manifest_is_present() {
    let dir = tempfile::tempdir().expect("an empty project dir");
    let config = Config::default();
    let args = publish_args();
    let opts = args.publish_options(&config, None, false);
    let client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let network = PublishNetwork { client: &client, auth_headers: &auth_headers };

    let err = args
        .publish_directory::<SilentReporter>(dir.path(), &config, &opts, &network)
        .await
        .expect_err("an empty directory has no package.json");

    assert_eq!(
        err.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND"),
    );
}

/// `--batch` is only meaningful with `--recursive`; passing it to a single
/// publish is rejected before any git check or network work.
#[tokio::test]
async fn run_rejects_batch_without_recursive() {
    let args = publish_args_with(PublishFlags { batch: true, ..publish_flags() });
    let err = args
        .run::<SilentReporter>(std::path::Path::new("."), &Config::default(), false)
        .await
        .expect_err("--batch requires --recursive");
    assert_eq!(
        err.code().map(|code| code.to_string()).as_deref(),
        Some("ERR_PNPM_BATCH_PUBLISH_REQUIRES_RECURSIVE"),
    );
}
