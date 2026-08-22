//! Custom fetchers on fresh and frozen CLI installs.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
    fixtures::{minimal_tarball, sha512_integrity},
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_env("XDG_CONFIG_HOME", workspace.join(".config"))
        .with_env("NO_PROXY", "127.0.0.1,localhost")
        .with_env("no_proxy", "127.0.0.1,localhost")
}

fn write_manifest(workspace: &Path, specifier: &str) {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": specifier,
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn custom_type_pnpmfile(registry_url: &str, with_fetcher: bool) -> String {
    let fetchers = if with_fetcher {
        r"fetchers: [{
    canFetch (pkgId, resolution) { return resolution.type === 'custom:e2e'; },
    fetch (cafs, resolution) {
      return { delegate: { tarball: resolution.url, integrity: resolution.integrity } };
    },
  }],"
    } else {
        ""
    };
    format!(
        r"module.exports = {{
  resolvers: [{{
    canResolve (wanted) {{ return wanted.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep'; }},
    async resolve () {{
      const response = await fetch('{registry_url}@pnpm.e2e%2Fdep-of-pkg-with-1-dep');
      const picked = (await response.json()).versions['100.1.0'];
      return {{
        id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
        manifest: picked,
        resolution: {{ type: 'custom:e2e', url: picked.dist.tarball, integrity: picked.dist.integrity }},
      }};
    }},
  }}],
  {fetchers}
}}",
    )
}

fn installed_version(workspace: &Path) -> String {
    let manifest_path = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read installed manifest"))
            .expect("parse installed manifest");
    manifest["version"].as_str().expect("version is a string").to_string()
}

#[test]
fn custom_fetcher_delegates_a_custom_typed_resolution_on_fresh_and_frozen_installs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "^100.0.0");
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("type: custom:e2e"),
        "lockfile records the custom-typed resolution: {lockfile}",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");
    drop((root, mock_instance));
}

#[test]
fn custom_typed_resolution_without_a_fetcher_fails_the_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "100.0.0");
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), false))
        .expect("write pnpmfile");

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains(r#"Cannot fetch dependency with custom resolution type "custom:e2e""#),
        "stderr: {stderr}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn ignore_pnpmfile_skips_the_custom_fetcher_on_fetch() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "100.0.0");
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");
    pacquet_at(&workspace).with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let output =
        pacquet_at(&workspace).with_args(["fetch", "--ignore-pnpmfile"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains(r#"Cannot fetch dependency with custom resolution type "custom:e2e""#),
        "stderr: {stderr}",
    );

    pacquet_at(&workspace).with_arg("fetch").assert().success();

    drop((root, mock_instance)); // cleanup
}

#[test]
fn ignore_pnpmfile_skips_the_custom_resolver_on_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, "100.0.0");
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");

    pacquet_at(&workspace).with_args(["install", "--ignore-pnpmfile"]).assert().success();
    assert_eq!(installed_version(&workspace), "100.0.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        !lockfile.contains("type: custom:e2e"),
        "the custom resolver must not reach the lockfile: {lockfile}",
    );

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");

    drop((root, mock_instance)); // cleanup
}

/// pnpm's `requireHooks` fails a configured pnpmfile that is not on disk with
/// `ERR_PNPM_PNPMFILE_NOT_FOUND`; only the default `.pnpmfile.cjs` is optional.
/// A generic execution failure here would read as a broken pnpmfile rather than
/// a misconfigured path.
#[test]
fn a_configured_pnpmfile_that_is_missing_names_itself() {
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let (metadata, original) = mock_fetcher_package(&mut registry, None);
    configure_fetcher_project(&workspace, &registry.url(), Some("absent.cjs"));

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_PNPMFILE_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("is not found"), "stderr: {stderr}");
    assert!(stderr.contains("absent.cjs"), "stderr: {stderr}");
    drop((metadata, original));
}

/// A configured path that names no module extension is checked with `.cjs`
/// appended, the way pnpm's `pnpmFileExistsSync` does. Such a path is present,
/// so whatever happens next is an execution failure — `require` resolves
/// `.js`/`.json`/`.node` but not `.cjs` — and reporting it as a missing file
/// would blame the setting for a pnpmfile that is right there on disk.
#[test]
fn an_extensionless_configured_pnpmfile_is_not_reported_as_missing() {
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let (metadata, original) = mock_fetcher_package(&mut registry, None);
    configure_fetcher_project(&workspace, &registry.url(), Some("hooks/custom"));
    fs::create_dir_all(workspace.join("hooks")).expect("create hooks dir");
    fs::write(workspace.join("hooks/custom.cjs"), "module.exports = { hooks: {} };\n")
        .expect("write pnpmfile");

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(!stderr.contains("ERR_PNPM_PNPMFILE_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("Error during pnpmfile execution"), "stderr: {stderr}");
    drop((metadata, original));
}

/// Only a configured path is required to exist. With the setting absent the
/// loader discovers `.pnpmfile.mjs` / `.pnpmfile.cjs`, and finding neither means
/// the project has no pnpmfile rather than a misconfiguration. An empty list is
/// configured-but-empty, reaching the same "no hooks" outcome down the other
/// branch, so both are pinned here.
#[test]
fn a_project_without_a_pnpmfile_installs() {
    for pnpmfile in [None, Some("[]")] {
        let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
        let (metadata, _original) =
            mock_fetcher_package(&mut registry, Some(&sha512_integrity(&tarball)));
        let archive = registry.mock("GET", "/original.tgz").with_body(tarball).create();
        configure_fetcher_project(&workspace, &registry.url(), pnpmfile);
        assert!(!workspace.join(".pnpmfile.mjs").exists());
        assert!(!workspace.join(".pnpmfile.cjs").exists());

        pacquet_at(&workspace).with_arg("install").assert().success();
        assert!(
            workspace.join("node_modules/fetcher-pkg/package.json").is_file(),
            "pnpmfile: {pnpmfile:?}",
        );
        drop((metadata, archive));
    }
}

fn configure_fetcher_project(workspace: &Path, registry: &str, pnpmfile: Option<&str>) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}/\n"))
        .expect("write registry configuration");
    // `None` leaves the setting out of the file, which is the only way to reach
    // the discovery branch: `pnpmfile: []` still counts as configured.
    let pnpmfile = pnpmfile.map_or_else(String::new, |value| format!("pnpmfile: {value}\n"));
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "registry: {registry}/\nstoreDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\n\
             fetchRetries: 0\n{pnpmfile}",
        ),
    )
    .expect("write workspace configuration");
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"fetcher-pkg":"1.0.0"}}"#)
        .expect("write package.json");
}

fn mock_fetcher_package(
    registry: &mut mockito::ServerGuard,
    integrity: Option<&str>,
) -> (mockito::Mock, mockito::Mock) {
    let mut dist = serde_json::json!({ "tarball": format!("{}/original.tgz", registry.url()) });
    if let Some(integrity) = integrity {
        dist["integrity"] = integrity.into();
    }
    let metadata = registry
        .mock("GET", "/fetcher-pkg")
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "name": "fetcher-pkg",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "name": "fetcher-pkg",
                        "version": "1.0.0",
                        "dist": dist,
                    },
                },
            })
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let original = registry.mock("GET", "/original.tgz").with_status(500).expect(0).create();
    (metadata, original)
}

fn fetcher_pnpmfile(body: &str) -> String {
    format!(
        r"const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
module.exports = {{ fetchers: [{{
  canFetch (pkgId) {{ return pkgId === 'fetcher-pkg@1.0.0'; }},
  async fetch (cafs, resolution, opts, fetchers) {{
    {body}
  }},
}}] }};
",
    )
}

#[test]
fn configured_fetchers_intercept_fresh_and_frozen_tarball_downloads() {
    let local_fetch = r"const temporary = await cafs.tempDir();
      const archive = path.join(temporary, 'package.tgz');
      try {
        fs.copyFileSync(path.join(__dirname, 'fixture.tgz'), archive);
        return await fetchers.localTarball(cafs, { tarball: 'file:' + path.relative(opts.lockfileDir, archive) }, opts);
      } finally {
        fs.rmSync(temporary, { recursive: true, force: true });
      }";
    let remote_fetch = r"assert.equal(resolution.fromDecliningFetcher, true);
      const server = require('node:https').createServer({
        key: fs.readFileSync(path.join(__dirname, 'tls.key')),
        cert: fs.readFileSync(path.join(__dirname, 'tls.crt')),
      }, (_, response) => response.end());
      await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
      try {
        await assert.rejects(
          fetchers.remoteTarball(cafs, { tarball: 'https://127.0.0.1:' + server.address().port + '/package.tgz' }, opts),
          error => /TLS|CERT/.test(error.code || '') && error.status == null && error.response?.status == null);
      } finally {
        await new Promise(resolve => server.close(resolve));
      }
      await assert.rejects(
        fetchers.remoteTarball(cafs, { tarball: resolution.tarball.replace('original.tgz', 'unavailable.tgz') }, opts),
        { code: 'ERR_PNPM_FETCH_503', status: 503, response: { status: 503 } });
      return fetchers.remoteTarball(cafs, { tarball: resolution.tarball.replace('original.tgz', 'custom.tgz') }, opts);";
    for (pnpmfile, fetch_body, pinned, expected_calls) in [
        ("configured.cjs", local_fetch, true, "fetch\n"),
        ("[declining.cjs, configured.cjs, unused.cjs]", remote_fetch, false, "decline\nfetch\n"),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
        let integrity = sha512_integrity(&tarball);
        let (metadata, original) =
            mock_fetcher_package(&mut registry, pinned.then_some(integrity.as_str()));
        let custom = registry
            .mock("GET", "/custom.tgz")
            .with_body(tarball.clone())
            .expect(if pinned { 0 } else { 2 })
            .create();
        let unavailable = registry
            .mock("GET", "/unavailable.tgz")
            .with_status(503)
            .expect(if pinned { 0 } else { 2 })
            .create();
        configure_fetcher_project(&workspace, &registry.url(), Some(pnpmfile));
        fs::write(workspace.join("fixture.tgz"), tarball).expect("write local tarball fixture");
        fs::write(
            workspace.join("tls.key"),
            include_bytes!("../../../network/tests/fixtures/test-client-pkcs1.key"),
        )
        .expect("write TLS key");
        fs::write(
            workspace.join("tls.crt"),
            include_bytes!("../../../network/tests/fixtures/test-client-pkcs1.crt"),
        )
        .expect("write TLS certificate");
        fs::write(
            workspace.join("declining.cjs"),
            r"const fs = require('node:fs');
const path = require('node:path');
module.exports = { fetchers: [{
  canFetch (pkgId, resolution) {
    if (pkgId === 'fetcher-pkg@1.0.0') {
      fs.appendFileSync(path.join(__dirname, 'calls'), 'decline\n');
      resolution.fromDecliningFetcher = true;
    }
    return false;
  },
  fetch () { throw new Error('declining fetcher was called'); },
}] };
",
        )
        .expect("write declining fetcher");
        fs::write(
            workspace.join("unused.cjs"),
            "module.exports = { fetchers: [{ canFetch () { throw new Error('fetcher order changed'); }, fetch () {} }] };\n",
        )
        .expect("write unused fetcher");
        let body = format!(
            r"fs.appendFileSync(path.join(__dirname, 'calls'), 'fetch\n');
    assert.equal(typeof cafs.storeDir, 'string');
    const result = await (async () => {{ {fetch_body} }})();
    assert.ok(result.filesMap instanceof Map);
    assert.equal(JSON.parse(fs.readFileSync(result.filesMap.get('package.json'), 'utf8')).version, '1.0.0');
    return result;",
        );
        fs::write(workspace.join("configured.cjs"), fetcher_pnpmfile(&body))
            .expect("write configured fetcher");

        for frozen in [false, true] {
            if frozen {
                fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
                fs::remove_dir_all(root.path().join("store")).expect("remove store");
                fs::remove_file(workspace.join("calls")).expect("remove first-install trace");
            }
            let args: &[&str] =
                if frozen { &["install", "--frozen-lockfile"] } else { &["install"] };
            pacquet_at(&workspace).with_args(args).assert().success();
            if !frozen {
                let lockfile = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
                    .expect("read lockfile")
                    .expect("lockfile exists");
                let key: pnpm_lockfile::PackageKey = "fetcher-pkg@1.0.0".parse().unwrap();
                let resolution = &lockfile.packages.as_ref().unwrap()[&key].resolution;
                assert_eq!(resolution.checkable_integrity().unwrap().to_string(), integrity);
                assert_eq!(
                    serde_json::to_value(resolution).unwrap()["tarball"],
                    format!("{}/original.tgz", registry.url()),
                );
            }
            let installed: serde_json::Value = serde_json::from_slice(
                &fs::read(workspace.join("node_modules/fetcher-pkg/package.json"))
                    .expect("read installed package"),
            )
            .expect("parse installed package");
            assert_eq!(installed["version"], "1.0.0", "{pnpmfile}, frozen={frozen}");
            assert_eq!(
                fs::read_to_string(workspace.join("calls")).expect("read fetcher trace"),
                expected_calls,
                "{pnpmfile}, frozen={frozen}",
            );
            if !frozen && !pinned {
                fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
                pacquet_at(&workspace)
                    .with_args(["install", "--frozen-lockfile", "--offline"])
                    .assert()
                    .success();
                assert_eq!(
                    fs::read_to_string(workspace.join("calls")).expect("read fetcher trace"),
                    expected_calls,
                    "a warm offline install must reuse the verified archive",
                );
            }
        }
        metadata.assert();
        original.assert();
        custom.assert();
        unavailable.assert();
    }
}

#[test]
fn declining_fetcher_rewrites_the_builtin_tarball_url() {
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
    let (metadata, original) =
        mock_fetcher_package(&mut registry, Some(&sha512_integrity(&tarball)));
    let mirror = registry.mock("GET", "/mirror.tgz").with_body(tarball).expect(1).create();
    configure_fetcher_project(&workspace, &registry.url(), Some("configured.cjs"));
    fs::write(
        workspace.join("configured.cjs"),
        r"module.exports = { fetchers: [{
  canFetch (pkgId, resolution) {
    resolution.tarball = resolution.tarball.replace('original.tgz', 'mirror.tgz');
    delete resolution.integrity;
    return false;
  },
  fetch () { throw new Error('declining fetcher was called'); },
}, {
  canFetch (_pkgId, resolution) {
    if (!resolution.integrity || !resolution.tarball.endsWith('/mirror.tgz')) throw new Error('declining mutation lost the locked integrity');
    return false;
  },
  fetch () { throw new Error('declining fetcher was called'); },
}] };",
    )
    .expect("write declining fetcher");
    pacquet_at(&workspace).with_arg("install").assert().success();
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/fetcher-pkg/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["version"], "1.0.0");
    metadata.assert();
    original.assert();
    mirror.assert();
}

/// Rust counterpart of the TypeScript picker's "cannot replace a locked archive
/// with a non-archive source": a hook that swaps a pinned tarball for a
/// directory while declining leaves nothing to verify the locked digest
/// against, so the install fails instead of importing unverified content.
#[test]
fn a_declining_fetcher_cannot_swap_a_locked_archive_for_a_directory() {
    let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
    let (metadata, original) =
        mock_fetcher_package(&mut registry, Some(&sha512_integrity(&tarball)));
    configure_fetcher_project(&workspace, &registry.url(), Some("configured.cjs"));
    fs::write(
        workspace.join("configured.cjs"),
        r"module.exports = { fetchers: [{
  canFetch (_pkgId, resolution) {
    resolution.type = 'directory';
    resolution.directory = '/synthetic/package';
    return false;
  },
  fetch () { throw new Error('declining fetcher was called'); },
}] };",
    )
    .expect("write declining fetcher");

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_TARBALL_INTEGRITY"), "stderr: {stderr}");
    assert!(
        !workspace.join("node_modules/fetcher-pkg/package.json").exists(),
        "a rewritten resolution must not import content",
    );
    metadata.assert();
    original.assert();
}

#[test]
fn custom_fetchers_cannot_replace_locked_integrity_or_return_unverified_files() {
    let changed_tarball = minimal_tarball("fetcher-pkg", "2.0.0");
    let changed_integrity = serde_json::to_string(&sha512_integrity(&changed_tarball)).unwrap();
    for (name, body, error) in [
        (
            "callback changes integrity",
            format!(
                "return fetchers.remoteTarball(cafs, {{ tarball: resolution.tarball.replace('original.tgz', 'changed.tgz'), integrity: {changed_integrity} }}, opts);",
            ),
            "integrity",
        ),
        (
            "delegate removes integrity",
            "return { delegate: { tarball: resolution.tarball.replace('original.tgz', 'changed.tgz') } };".to_string(),
            "integrity",
        ),
        (
            "remote callback given a non-http scheme",
            "return fetchers.remoteTarball(cafs, { tarball: 'ftp://example.invalid/pkg.tgz' }, opts);".to_string(),
            "incompatible url",
        ),
        (
            "remote callback given a bare path",
            "return fetchers.remoteTarball(cafs, { tarball: '../../../etc/passwd' }, opts);".to_string(),
            "incompatible url",
        ),
        (
            "fabricated map",
            "return { filesMap: new Map([['package.json', path.join(__dirname, 'package.json')]]), requiresBuild: false };".to_string(),
            "not verified by a native tarball fetcher",
        ),
        (
            "modified callback map",
            "const result = await fetchers.remoteTarball(cafs, { ...resolution, tarball: resolution.tarball.replace('original.tgz', 'custom.tgz') }, opts); result.filesMap.set('injected.json', result.filesMap.get('package.json')); return result;".to_string(),
            "not verified by a native tarball fetcher",
        ),
    ] {
        let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
        let (metadata, original) =
            mock_fetcher_package(&mut registry, Some(&sha512_integrity(&tarball)));
        let _changed = registry.mock("GET", "/changed.tgz").with_body(changed_tarball.clone()).create();
        let custom = registry
            .mock("GET", "/custom.tgz")
            .with_body(tarball)
            .expect(usize::from(name == "modified callback map"))
            .create();
        configure_fetcher_project(&workspace, &registry.url(), Some("configured.cjs"));
        fs::write(workspace.join("configured.cjs"), fetcher_pnpmfile(&body))
            .expect("write rejecting fetcher");

        let output = pacquet_at(&workspace).with_arg("install").assert().failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        assert!(stderr.to_ascii_lowercase().contains(error), "{name}: {stderr}");
        assert!(
            !workspace.join("node_modules/fetcher-pkg/package.json").exists(),
            "{name} imported unverified content",
        );
        metadata.assert();
        original.assert();
        custom.assert();
    }
}
