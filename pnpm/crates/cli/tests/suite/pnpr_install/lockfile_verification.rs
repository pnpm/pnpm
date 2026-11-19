use std::{fs, io::Write as _};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::get_all_files,
};

use super::{configure_pnpr_auth, pacquet_at, pnpr_done_frame, start_pnpr};

fn build_tarball_with_postinstall(name: &str, version: &str) -> (Vec<u8>, String) {
    let manifest = serde_json::json!({
        "name": name,
        "version": version,
        "scripts": {
            "postinstall": r#"node -e "require('fs').writeFileSync(process.env.MARKER_FILE, 'ran-script')""#,
        },
    })
    .to_string();
    let mut tar_builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar_builder
        .append_data(&mut header, "package/package.json", manifest.as_bytes())
        .expect("append package.json");
    let tar_bytes = tar_builder.into_inner().expect("finish tar");
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar_bytes).expect("compress tar");
    let tgz = gzip.finish().expect("finish gzip");
    let integrity = ssri::Integrity::from(&tgz).to_string();
    (tgz, integrity)
}

fn shape_mismatched_lockfile_yaml(name: &str, version: &str, integrity: &str) -> String {
    format!(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      '{name}':
        specifier: {version}
        version: {version}
packages:
  '{name}@{version}':
    resolution: {{integrity: {integrity}, tarball: 'file:alt.tgz'}}
snapshots:
  '{name}@{version}': {{}}
",
    )
}

#[test]
fn frozen_install_via_pnpr_rejects_shape_mismatch_without_running_builds_or_caching() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        npmrc_path,
        cache_dir,
        mock_instance,
        ..
    } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let (alt_tgz, alt_integrity) = build_tarball_with_postinstall("@foo/no-deps", "1.0.0");
    fs::write(workspace.join("alt.tgz"), alt_tgz).expect("write alt.tgz");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"@foo/no-deps":"1.0.0"}}"#,
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-lock.yaml"),
        shape_mismatched_lockfile_yaml("@foo/no-deps", "1.0.0", &alt_integrity),
    )
    .expect("write tampered pnpm-lock.yaml");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "pnprServer: {pnpr_url}\nminimumReleaseAge: 0\nallowBuilds:\n  '@foo/no-deps': true\n",
        ),
    )
    .expect("write pnpm-workspace.yaml");

    let pnpr_marker = root.path().join("marker-pnpr");
    let assert = pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("MARKER_FILE", &pnpr_marker)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&assert.get_output().stdout),
        String::from_utf8_lossy(&assert.get_output().stderr),
    );
    assert!(
        output.contains("ERR_PNPM_RESOLUTION_SHAPE_MISMATCH"),
        "expected ERR_PNPM_RESOLUTION_SHAPE_MISMATCH, got:\n{output}",
    );
    assert!(!pnpr_marker.exists(), "postinstall script must never run");
    if cache_dir.exists() {
        let cached_files = get_all_files(&cache_dir);
        assert!(
            !cached_files
                .iter()
                .any(|path| path.contains("lockfile-verified.jsonl")),
            "must not write lockfile-verified.jsonl, found: {cached_files:?}",
        );
    }

    // `--trust-lockfile` opts out only of online supply-chain policies, never offline shape checks.
    let trusted_assert = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("MARKER_FILE", &pnpr_marker)
        .with_args(["install", "--frozen-lockfile", "--trust-lockfile"])
        .assert()
        .failure();
    let trusted_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&trusted_assert.get_output().stdout),
        String::from_utf8_lossy(&trusted_assert.get_output().stderr),
    );
    assert!(
        trusted_output.contains("ERR_PNPM_RESOLUTION_SHAPE_MISMATCH"),
        "expected ERR_PNPM_RESOLUTION_SHAPE_MISMATCH under --trust-lockfile, got:\n{trusted_output}",
    );
    assert!(!pnpr_marker.exists(), "postinstall script must never run under --trust-lockfile");

    // Subsequent local frozen install without pnprServer must also fail without running postinstall.
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 0\nallowBuilds:\n  '@foo/no-deps': true\n",
    )
    .expect("write local pnpm-workspace.yaml");
    let local_marker = root.path().join("marker-local");
    let local_assert = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_env("MARKER_FILE", &local_marker)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let local_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&local_assert.get_output().stdout),
        String::from_utf8_lossy(&local_assert.get_output().stderr),
    );
    assert!(
        local_output.contains("ERR_PNPM_RESOLUTION_SHAPE_MISMATCH"),
        "expected local ERR_PNPM_RESOLUTION_SHAPE_MISMATCH, got:\n{local_output}",
    );
    assert!(!local_marker.exists(), "local postinstall script must never run");

    drop((root, mock_instance));
}

#[test]
fn client_rejects_shape_mismatched_lockfile_from_permissive_pnpr_server() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { cache_dir, mock_instance, .. } = npmrc_info;

    let (alt_tgz, alt_integrity) = build_tarball_with_postinstall("@foo/no-deps", "1.0.0");
    fs::write(workspace.join("alt.tgz"), alt_tgz).expect("write alt.tgz");
    fs::write(
        workspace.join("package.json"),
        r#"{"name":"app","version":"1.0.0","dependencies":{"@foo/no-deps":"1.0.0"}}"#,
    )
    .expect("write package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "minimumReleaseAge: 0\nallowBuilds:\n  '@foo/no-deps': true\n",
    )
    .expect("write pnpm-workspace.yaml");

    let mut malicious_server = mockito::Server::new();
    let resolve_mock = malicious_server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body_from_request(move |request| {
            let lockfile: serde_json::Value = serde_saphyr::from_str(
                &shape_mismatched_lockfile_yaml("@foo/no-deps", "1.0.0", &alt_integrity),
            )
            .expect("tampered lockfile parses");
            pnpr_done_frame(
                request,
                serde_json::json!({
                    "type": "done",
                    "lockfile": lockfile,
                    "stats": { "totalPackages": 1 },
                }),
            )
        })
        .expect(2)
        .create();

    let marker = root.path().join("marker-malicious-server");
    for extra_flag in [None, Some("--trust-lockfile")] {
        let mut cmd = pacquet_at(&workspace);
        cmd.env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .env("MARKER_FILE", &marker)
            .args(["install", "--pnpr-server", &malicious_server.url()]);
        if let Some(flag) = extra_flag {
            cmd.arg(flag);
        }
        let assert = cmd.assert().failure();
        let output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&assert.get_output().stdout),
            String::from_utf8_lossy(&assert.get_output().stderr),
        );
        assert!(
            output.contains("ERR_PNPM_RESOLUTION_SHAPE_MISMATCH"),
            "expected ERR_PNPM_RESOLUTION_SHAPE_MISMATCH ({extra_flag:?}), got:\n{output}",
        );
        assert!(!marker.exists(), "postinstall must not run ({extra_flag:?})");
        assert!(
            !workspace.join("pnpm-lock.yaml").exists(),
            "tampered lockfile must not be saved ({extra_flag:?})",
        );
    }
    resolve_mock.assert();
    let _ = pacquet;
    if cache_dir.exists() {
        let cached_files = get_all_files(&cache_dir);
        assert!(
            !cached_files
                .iter()
                .any(|path| path.contains("lockfile-verified.jsonl")),
            "must not write lockfile-verified.jsonl, found: {cached_files:?}",
        );
    }

    drop((root, mock_instance));
}
