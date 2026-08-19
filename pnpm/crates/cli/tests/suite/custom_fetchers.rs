//! Custom fetchers from a `.pnpmfile.cjs` `fetchers` export, end to
//! end: a custom resolver writes a custom-typed resolution into the
//! lockfile, and the sibling fetcher claims it and delegates to the
//! built-in tarball path — on the fresh-resolve install, on a frozen
//! reinstall, and failing loudly when the fetcher is removed.

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

fn write_manifest(workspace: &Path) {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

#[derive(Clone, Copy)]
enum CustomTypeFetcher {
    Absent,
    Delegate,
    RemoteTarball,
}

fn custom_type_pnpmfile(registry_url: &str, fetcher: CustomTypeFetcher) -> String {
    let (fetch_body, integrity_field) = match fetcher {
        CustomTypeFetcher::Absent => (None, "integrity"),
        CustomTypeFetcher::Delegate => (
            Some(
                "return { delegate: { tarball: resolution.url, integrity: resolution.integrity } };",
            ),
            "integrity",
        ),
        CustomTypeFetcher::RemoteTarball => (
            Some(
                "if (resolution.integrity !== undefined) throw new Error('resolution already has integrity');\n\
                 const result = await fetchers.remoteTarball(cafs, { tarball: resolution.url }, opts);\n\
                 if (result.integrity !== resolution.expectedIntegrity) throw new Error('computed integrity differs from archive');\n\
                 return result;",
            ),
            "expectedIntegrity",
        ),
    };
    let fetchers = fetch_body.map_or_else(String::new, |fetch_body| {
        format!(
            r"
  fetchers: [
    {{
      canFetch (pkgId, resolution) {{
        return resolution.type === 'custom:e2e';
      }},
      async fetch (cafs, resolution, opts, fetchers) {{
        {fetch_body}
      }},
    }},
  ],
",
        )
    });
    format!(
        r"module.exports = {{
  resolvers: [
    {{
      canResolve (wanted) {{
        return wanted.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep';
      }},
      async resolve () {{
        const response = await fetch('{registry_url}@pnpm.e2e%2Fdep-of-pkg-with-1-dep');
        const meta = await response.json();
        const picked = meta.versions['100.1.0'];
        return {{
          id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
          manifest: picked,
          resolution: {{
            type: 'custom:e2e',
            url: picked.dist.tarball,
            {integrity_field}: picked.dist.integrity,
          }},
        }};
      }},
    }},
  ],{fetchers}
}}
",
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
    for fetcher in [CustomTypeFetcher::Delegate, CustomTypeFetcher::RemoteTarball] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        write_manifest(&workspace);
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            custom_type_pnpmfile(&mock_instance.url(), fetcher),
        )
        .expect("write pnpmfile");

        // Fresh resolve: the custom resolver writes the custom-typed
        // resolution, and the fetcher must delegate it during the same
        // install's fetch phase.
        pacquet_at(&workspace).with_arg("install").assert().success();
        assert_eq!(installed_version(&workspace), "100.1.0");
        let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
        assert!(
            lockfile.contains("type: custom:e2e"),
            "lockfile records the custom-typed resolution: {lockfile}",
        );

        // Frozen reinstall: the lockfile is the source of truth now, so the
        // fetcher (loaded by the frozen path) is the only way to
        // materialize the custom-typed entry.
        fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
        pacquet_at(&workspace).with_arg("install").assert().success();
        assert_eq!(installed_version(&workspace), "100.1.0");

        drop((root, mock_instance)); // cleanup
    }
}

#[test]
fn custom_typed_resolution_without_a_fetcher_fails_the_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        custom_type_pnpmfile(&mock_instance.url(), CustomTypeFetcher::Absent),
    )
    .expect("write pnpmfile");

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains(r#"Cannot fetch dependency with custom resolution type "custom:e2e""#),
        "stderr: {stderr}",
    );

    drop((root, mock_instance)); // cleanup
}

/// `pnpm fetch` takes `--ignore-pnpmfile` too, and its frozen path is
/// the one that loads the custom fetchers. Without a fetcher the
/// custom-typed resolution the lockfile records cannot be materialized,
/// so the flag turns a working fetch into the same loud failure a
/// missing `fetchers` export produces.
#[test]
fn ignore_pnpmfile_skips_the_custom_fetcher_on_fetch() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        custom_type_pnpmfile(&mock_instance.url(), CustomTypeFetcher::Delegate),
    )
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

    // The same fetch with the pnpmfile honored, so the assertion above
    // cannot pass on a fixture that could never fetch.
    pacquet_at(&workspace).with_arg("fetch").assert().success();

    drop((root, mock_instance)); // cleanup
}

/// The resolver half of the same pnpmfile: with `--ignore-pnpmfile` the
/// custom resolver never claims the dependency, so the install resolves
/// the version the manifest asks for instead of the one the resolver
/// substitutes.
#[test]
fn ignore_pnpmfile_skips_the_custom_resolver_on_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        custom_type_pnpmfile(&mock_instance.url(), CustomTypeFetcher::Delegate),
    )
    .expect("write pnpmfile");

    pacquet_at(&workspace).with_args(["install", "--ignore-pnpmfile"]).assert().success();
    assert_eq!(installed_version(&workspace), "100.0.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        !lockfile.contains("type: custom:e2e"),
        "the custom resolver must not reach the lockfile: {lockfile}",
    );

    // Resolve again with the pnpmfile honored, so the assertions above
    // cannot pass on a fixture whose resolver never worked.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");

    drop((root, mock_instance)); // cleanup
}

fn configure_fetcher_project(workspace: &Path, registry: &str, pnpmfile: &str) {
    fs::write(workspace.join(".npmrc"), format!("registry={registry}/\n"))
        .expect("write registry configuration");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!(
            "registry: {registry}/\nstoreDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\n\
             fetchRetries: 0\npnpmfile: {pnpmfile}\n",
        ),
    )
    .expect("write workspace configuration");
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"fetcher-pkg":"1.0.0"}}"#)
        .expect("write package.json");
}

fn mock_fetcher_package(
    registry: &mut mockito::ServerGuard,
    integrity: &str,
) -> (mockito::Mock, mockito::Mock) {
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
                        "dist": {
                            "integrity": integrity,
                            "tarball": format!("{}/original.tgz", registry.url()),
                        },
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
        r"const fs = require('node:fs');
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
    for (pnpmfile, method, expected_calls) in [
        ("configured.cjs", "localTarball", "fetch\n"),
        ("[declining.cjs, configured.cjs, unused.cjs]", "remoteTarball", "decline\nfetch\n"),
    ] {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
        let integrity = sha512_integrity(&tarball);
        let (metadata, original) = mock_fetcher_package(&mut registry, &integrity);
        let custom = registry
            .mock("GET", "/custom.tgz")
            .with_body(tarball.clone())
            .expect(if method == "remoteTarball" { 2 } else { 0 })
            .create();
        let unavailable = registry
            .mock("GET", "/unavailable.tgz")
            .with_status(503)
            .expect(if method == "remoteTarball" { 2 } else { 0 })
            .create();
        configure_fetcher_project(&workspace, &registry.url(), pnpmfile);
        fs::write(workspace.join("fixture.tgz"), tarball).expect("write local tarball fixture");
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
    if ('{method}' === 'remoteTarball' && !resolution.fromDecliningFetcher) throw new Error('declining fetcher mutation was lost');
    if (typeof cafs.storeDir !== 'string') throw new Error('CAFS storeDir is missing');
    const temporary = '{method}' === 'localTarball' ? await cafs.tempDir() : null;
    const archive = temporary && path.join(temporary, 'package.tgz');
    if (archive) fs.copyFileSync(path.join(__dirname, 'fixture.tgz'), archive);
    const tarball = {tarball};
    try {{
      if ('{method}' === 'remoteTarball') {{
        const server = require('node:https').createServer({{ key: {tls_key}, cert: {tls_cert} }}, (_, response) => response.end());
        await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
        try {{
          await fetchers.remoteTarball(cafs, {{ tarball: 'https://127.0.0.1:' + server.address().port + '/package.tgz' }}, opts);
          throw new Error('expected TLS certificate rejection');
        }} catch (error) {{
          if (!/TLS|CERT/.test(error.code || '') || error.status != null || error.response?.status != null) throw error;
        }} finally {{
          await new Promise((resolve) => server.close(resolve));
        }}
        try {{
          await fetchers.remoteTarball(cafs, {{ tarball: tarball.replace('custom.tgz', 'unavailable.tgz') }}, opts);
          throw new Error('expected HTTP 503');
        }} catch (error) {{
          if (error.code !== 'ERR_PNPM_FETCH_503' || error.status !== 503 || error.response?.status !== 503) throw error;
        }}
      }}
      const result = await fetchers.{method}(cafs, {{ tarball }}, opts);
      if (!(result.filesMap instanceof Map)) throw new Error('filesMap must be a Map');
      const manifest = JSON.parse(fs.readFileSync(result.filesMap.get('package.json'), 'utf8'));
      if (manifest.version !== '1.0.0') throw new Error('callback returned before extraction');
      return result;
    }} finally {{
      if (temporary) fs.rmSync(temporary, {{ recursive: true, force: true }});
    }}",
            tarball = if method == "localTarball" {
                "'file:' + path.relative(opts.lockfileDir, archive)".to_string()
            } else {
                serde_json::to_string(&format!("{}/custom.tgz", registry.url())).unwrap()
            },
            tls_key = serde_json::to_string(include_str!(
                "../../../network/tests/fixtures/test-client-pkcs1.key"
            ))
            .unwrap(),
            tls_cert = serde_json::to_string(include_str!(
                "../../../network/tests/fixtures/test-client-pkcs1.crt"
            ))
            .unwrap(),
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
                let lockfile =
                    fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
                assert!(
                    !lockfile.contains("pnpmfileChecksum:"),
                    "fetcher-only pnpmfiles must not change the hooks checksum: {lockfile}",
                );
            }
            let installed: serde_json::Value = serde_json::from_slice(
                &fs::read(workspace.join("node_modules/fetcher-pkg/package.json"))
                    .expect("read installed package"),
            )
            .expect("parse installed package");
            assert_eq!(installed["version"], "1.0.0", "{method}, frozen={frozen}");
            assert_eq!(
                fs::read_to_string(workspace.join("calls")).expect("read fetcher trace"),
                expected_calls,
                "{method}, frozen={frozen}",
            );
        }
        metadata.assert();
        original.assert();
        custom.assert();
        unavailable.assert();
    }
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
            "callback removes integrity",
            "return fetchers.remoteTarball(cafs, { tarball: resolution.tarball.replace('original.tgz', 'changed.tgz') }, opts);".to_string(),
            "integrity",
        ),
        (
            "delegate changes integrity",
            format!(
                "return {{ delegate: {{ tarball: resolution.tarball.replace('original.tgz', 'changed.tgz'), integrity: {changed_integrity} }} }};",
            ),
            "integrity",
        ),
        (
            "delegate removes integrity",
            "return { delegate: { tarball: resolution.tarball.replace('original.tgz', 'changed.tgz') } };".to_string(),
            "integrity",
        ),
        (
            "fabricated map",
            "return { filesMap: new Map([['package.json', path.join(__dirname, 'package.json')]]), requiresBuild: false };".to_string(),
            "files",
        ),
        (
            "modified callback map",
            "const result = await fetchers.remoteTarball(cafs, { ...resolution, tarball: resolution.tarball.replace('original.tgz', 'custom.tgz') }, opts); result.filesMap.set('injected.json', result.filesMap.get('package.json')); return result;".to_string(),
            "files",
        ),
    ] {
        let CommandTempCwd { root: _root, workspace, .. } = CommandTempCwd::init();
        let mut registry = mockito::Server::new();
        let tarball = minimal_tarball("fetcher-pkg", "1.0.0");
        let (metadata, original) = mock_fetcher_package(&mut registry, &sha512_integrity(&tarball));
        let _changed = registry.mock("GET", "/changed.tgz").with_body(changed_tarball.clone()).create();
        let custom = registry
            .mock("GET", "/custom.tgz")
            .with_body(tarball)
            .expect(usize::from(name == "modified callback map"))
            .create();
        configure_fetcher_project(&workspace, &registry.url(), "configured.cjs");
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
