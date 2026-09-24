//! Custom resolvers from a `.pnpmfile.cjs` `resolvers` export: chain
//! precedence over the built-in resolvers, `shouldRefreshResolution`
//! forcing re-resolution past the prefer-frozen fast path, and hook
//! failures aborting the install. Mirrors pnpm's
//! `installing/deps-installer/test/install/customResolvers.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path) {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A resolver that claims `@pnpm.e2e/dep-of-pkg-with-1-dep` and resolves
/// it to `100.1.0` — a version the built-in npm resolver would never pick
/// for the exact `100.0.0` specifier, so its presence in the result
/// proves the custom resolver ran.
fn overriding_pnpmfile(registry_url: &str, should_refresh: &str) -> String {
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
        const dist = meta.versions['100.1.0'].dist;
        return {{
          id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
          resolution: {{ tarball: dist.tarball, integrity: dist.integrity }},
        }};
      }},
      shouldRefreshResolution () {{
        return {should_refresh};
      }},
    }},
  ],
}}
",
    )
}

/// `manifest` is optional in a resolver's result, so omitting it leaves
/// the fetched tarball as the only source of the package's own
/// dependencies.
fn manifest_less_pnpmfile(registry_url: &str) -> String {
    format!(
        r"module.exports = {{
  resolvers: [
    {{
      canResolve (wanted) {{
        return wanted.alias === '@pnpm.e2e/pkg-with-1-dep';
      }},
      async resolve () {{
        const response = await fetch('{registry_url}@pnpm.e2e%2Fpkg-with-1-dep');
        const meta = await response.json();
        const dist = meta.versions['100.0.0'].dist;
        return {{
          id: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
          resolution: {{ tarball: dist.tarball, integrity: dist.integrity }},
        }};
      }},
    }},
  ],
}}
",
    )
}

fn installed_version(workspace: &Path) -> String {
    let manifest_path = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read installed manifest"))
            .expect("parse installed manifest");
    manifest["version"]
        .as_str()
        .expect("version is a string")
        .to_string()
}

#[test]
fn custom_resolver_takes_precedence_over_builtin_resolvers() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), overriding_pnpmfile(mock_instance.url(), "false"))
        .expect("write pnpmfile");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "lockfile records the custom resolution: {lockfile}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn should_refresh_resolution_forces_re_resolution_past_the_frozen_path() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    // First install: the resolver implements only
    // `shouldRefreshResolution`, so it never joins the resolver chain
    // and the built-in npm resolver pins 100.0.0.
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution: () => false }] }\n",
    )
    .expect("write pnpmfile");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed_version(&workspace), "100.0.0");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/dep-of-pkg-with-1-dep": ">=100.0.0 <101",
            },
        })
        .to_string(),
    )
    .expect("update dependency range");

    // Second install: the changed range still accepts the locked version,
    // so without the hook the install would keep that resolution.
    // `shouldRefreshResolution` returning true must force the
    // fresh-resolve path, where the custom resolver now overrides the
    // pinned version.
    fs::write(workspace.join(".pnpmfile.cjs"), overriding_pnpmfile(mock_instance.url(), "true"))
        .expect("rewrite pnpmfile");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "lockfile records the refreshed resolution: {lockfile}",
    );
    assert!(
        lockfile.contains("specifier: '>=100.0.0 <101'"),
        "lockfile records the updated range: {lockfile}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn failing_should_refresh_resolution_aborts_the_install() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution: () => false }] }\n",
    )
    .expect("write pnpmfile");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution () { throw new Error('refresh check crashed'); } }] }\n",
    )
    .expect("rewrite pnpmfile");

    let output = pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    // miette wraps the report at the terminal width, and where the wrap
    // falls depends on the temp-dir path length in the message, so the
    // phrase is matched with the wrapping collapsed.
    let unwrapped = stderr
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(unwrapped.contains("refresh check crashed"), "stderr: {stderr}");

    drop((root, mock_instance)); // cleanup
}

/// Port of upstream's `'custom resolver receives currentPkg on
/// subsequent installs'` (`installing/deps-installer/test/install/customResolvers.ts`).
///
/// The first install records the npm resolver's pick — a `Registry`
/// (`{integrity}`-only) lockfile entry. The second install is forced to
/// re-resolve via `shouldRefreshResolution`; the resolver must receive
/// that entry as `currentPkg` with the tarball URL re-derived from the
/// registry, and echoing it back must keep the pinned version.
#[test]
fn custom_resolver_receives_current_pkg_on_subsequent_installs() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    // First install: the resolver claims nothing, so the npm resolver
    // pins 100.0.0 and the lockfile compacts it to `{integrity}`.
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution: () => false }] }\n",
    )
    .expect("write pnpmfile");
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed_version(&workspace), "100.0.0");

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"const fs = require('node:fs');
const path = require('node:path');
module.exports = {
  resolvers: [
    {
      canResolve (wanted) {
        return wanted.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep';
      },
      resolve (wanted, opts) {
        fs.writeFileSync(path.join(opts.lockfileDir, 'resolver-opts.json'), JSON.stringify(opts));
        if (!opts.currentPkg) {
          throw new Error('expected currentPkg on the second install');
        }
        return { id: opts.currentPkg.id, resolution: opts.currentPkg.resolution };
      },
      shouldRefreshResolution: () => true,
    },
  ],
}
",
    )
    .expect("rewrite pnpmfile");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    assert_eq!(installed_version(&workspace), "100.0.0", "echoing currentPkg keeps the pin");
    let opts: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("resolver-opts.json")).expect("resolver dumped opts"),
    )
    .expect("parse dumped opts");
    let current_pkg = &opts["currentPkg"];
    assert_eq!(current_pkg["id"], "@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0");
    assert_eq!(current_pkg["name"], "@pnpm.e2e/dep-of-pkg-with-1-dep");
    assert_eq!(current_pkg["version"], "100.0.0");
    // The on-disk entry is `{integrity}`-only; the payload must carry
    // the tarball URL re-derived from the registry, like pnpm's
    // `pkgSnapshotToResolution`.
    let tarball = current_pkg["resolution"]["tarball"].as_str().expect("derived tarball URL");
    assert!(
        tarball.ends_with("/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz"),
        "got: {tarball}",
    );
    assert!(
        current_pkg["resolution"]["integrity"].is_string(),
        "the recorded integrity carries over: {current_pkg}",
    );

    drop((root, mock_instance)); // cleanup
}

/// Regression test for pnpm/pnpm#15000.
#[test]
fn custom_resolver_without_a_manifest_installs_the_package_with_its_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join(".pnpmfile.cjs"), manifest_less_pnpmfile(mock_instance.url()))
        .expect("write pnpmfile");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let dependency = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep",
    );
    assert!(dependency.is_dir(), "the resolved package's own dependency is installed");

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "the lockfile records the dependency read from the tarball: {lockfile}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn custom_resolver_local_tarball_without_manifest_installs_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest = serde_json::json!({
        "name": "custom-local", "version": "1.0.0",
        "dependencies": {"@pnpm.e2e/dep-of-pkg-with-1-dep": "100.1.0"},
    });
    let body = pnpm_testing_utils::fixtures::tarball_with_manifest(&manifest);
    fs::create_dir_all(workspace.join("vendor")).unwrap();
    fs::write(workspace.join("vendor/package.tgz"), &body).unwrap();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"custom-local":"1.0.0"}}"#)
        .unwrap();
    let resolution = serde_json::json!({
        "tarball": "file:./vendor/package.tgz",
        "integrity": pnpm_testing_utils::fixtures::sha512_integrity(&body),
    });
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{ resolvers: [{{
  canResolve: wanted => wanted.alias === 'custom-local',
  resolve: () => ({{ id: 'custom-local@1.0.0', resolution: {resolution} }}),
}}] }};
",
        ),
    )
    .unwrap();
    pacquet
        .with_arg("install")
        .assert()
        .success();
    pacquet_at(&workspace).with_args(["exec", "node", "-e",
        "console.log(require(require.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep/package.json', { paths: [require.resolve('custom-local/package.json')] })).version)"])
        .assert().success().stdout("100.1.0\n");
    drop((root, mock_instance));
}

/// A custom resolution records no location, so only the fetcher that claims it
/// can reach the archive the package's own dependencies are declared in.
/// Regression test for pnpm/pnpm#15552.
#[test]
fn custom_typed_resolver_without_manifest_installs_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest = serde_json::json!({
        "name": "custom-typed", "version": "1.0.0",
        "dependencies": {"@pnpm.e2e/dep-of-pkg-with-1-dep": "100.1.0"},
    });
    let body = pnpm_testing_utils::fixtures::tarball_with_manifest(&manifest);
    fs::create_dir_all(workspace.join("vendor")).unwrap();
    fs::write(workspace.join("vendor/package.tgz"), &body).unwrap();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"custom-typed":"1.0.0"}}"#)
        .unwrap();
    let integrity = pnpm_testing_utils::fixtures::sha512_integrity(&body);
    let resolution = serde_json::json!({
        "type": "custom:vendored", "name": "custom-typed", "version": "1.0.0",
        "integrity": integrity,
    });
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{
  resolvers: [{{
    canResolve: wanted => wanted.alias === 'custom-typed',
    resolve: () => ({{ id: 'custom-typed@1.0.0', resolution: {resolution} }}),
  }}],
  fetchers: [{{
    canFetch: (id, resolution) => {{
      // hook-local scratch, not a lockfile field
      resolution._localCache = process.cwd() + '/.cache/' + id;
      return resolution.type === 'custom:vendored';
    }},
    fetch: (cafs, resolution, opts, fetchers) => fetchers.localTarball(
      cafs,
      {{ tarball: 'file:./vendor/package.tgz', integrity: '{integrity}' }},
      opts,
    ),
  }}],
}};
",
        ),
    )
    .unwrap();
    pacquet
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let dependency_version = |workspace: &Path| {
        pacquet_at(workspace).with_args(["exec", "node", "-e",
            "console.log(require(require.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep/package.json', { paths: [require.resolve('custom-typed/package.json')] })).version)"])
            .assert().success().stdout("100.1.0\n");
    };
    dependency_version(&workspace);

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    // The resolution stays the resolver's, so the lockfile keeps naming no
    // location and the same fetcher claims it on the next install. A field the
    // fetcher's `canFetch` left behind is the fetcher's, not the resolver's,
    // and committing it would make the lockfile machine-dependent.
    assert!(
        lockfile.contains("type: custom:vendored") && !lockfile.contains("tarball:"),
        "the custom resolution is recorded verbatim: {lockfile}",
    );
    assert!(
        !lockfile.contains("_localCache"),
        "no scratch field from canFetch reaches the lockfile: {lockfile}",
    );

    // A frozen install has only the lockfile to work from, so an empty snapshot
    // recorded above would silently install the package without its dependency.
    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();
    dependency_version(&workspace);

    drop((root, mock_instance));
}

/// A fetcher that hands a custom resolution back to the registry names only
/// the integrity. The archive to read the package's dependencies from is then
/// the registry tarball of the package the resolver's id names.
#[test]
fn custom_resolution_delegated_to_the_registry_installs_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let registry_url = mock_instance.url();
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{
  resolvers: [{{
    canResolve: wanted => wanted.alias === '@pnpm.e2e/pkg-with-1-dep',
    async resolve () {{
      const response = await fetch('{registry_url}@pnpm.e2e%2Fpkg-with-1-dep');
      const dist = (await response.json()).versions['100.0.0'].dist;
      return {{
        id: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
        resolution: {{ type: 'custom:registry', integrity: dist.integrity }},
      }};
    }},
  }}],
  fetchers: [{{
    canFetch: (id, resolution) => resolution.type === 'custom:registry',
    fetch: (cafs, resolution) => ({{ delegate: {{ integrity: resolution.integrity }} }}),
  }}],
}};
",
        ),
    )
    .unwrap();
    pacquet
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let dependency_version = |workspace: &Path| {
        pacquet_at(workspace).with_args(["exec", "node", "-e",
            "console.log(require(require.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep/package.json', { paths: [require.resolve('@pnpm.e2e/pkg-with-1-dep/package.json')] })).version)"])
            .assert().success().stdout("100.1.0\n");
    };
    dependency_version(&workspace);

    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();
    dependency_version(&workspace);

    drop((root, mock_instance));
}

#[test]
fn custom_resolver_git_subdirectory_installs_its_manifest_and_dependencies() {
    assert_git_subdirectory_install(
        "return { delegate: { ...resolution, tarball: 'file:./repo.tgz' } };",
    );
}

#[test]
fn custom_resolver_git_subdirectory_installs_custom_fetched_files() {
    assert_git_subdirectory_install(
        "return fetchers.localTarball(cafs, { ...resolution, tarball: 'file:./repo.tgz' }, opts);",
    );
}

fn assert_git_subdirectory_install(fetch_body: &str) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest = serde_json::json!({
        "name": "custom-git", "version": "1.0.0", "files": ["index.js"],
        "dependencies": {"@pnpm.e2e/dep-of-pkg-with-1-dep": "100.1.0"},
    })
    .to_string();
    let body = pnpm_testing_utils::fixtures::tarball_entries(&[
        ("repo/package.json", br#"{"name":"archive-root","version":"1.0.0"}"#),
        ("repo/packages/foo/package.json", manifest.as_bytes()),
        ("repo/packages/foo/index.js", b"module.exports = 42;"),
        ("repo/packages/foo/excluded.txt", b"must not be installed"),
    ]);
    fs::write(workspace.join("repo.tgz"), &body).unwrap();
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"custom-git":"1.0.0"}}"#)
        .unwrap();
    let resolution = serde_json::json!({
        "tarball": "https://codeload.github.com/example/repo/tar.gz/0123456789abcdef0123456789abcdef01234567",
        "integrity": pnpm_testing_utils::fixtures::sha512_integrity(&body),
        "path": "/packages/foo",
        "gitHosted": true,
    });
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{
  resolvers: [{{
    canResolve: wanted => wanted.alias === 'custom-git',
    resolve: () => ({{ id: 'custom-git@1.0.0', resolution: {resolution} }}),
  }}],
  fetchers: [{{
    canFetch: (id, resolution) => resolution.gitHosted === true,
    async fetch(cafs, resolution, opts, fetchers) {{ {fetch_body} }},
  }}],
}};
",
        ),
    )
    .unwrap();
    pacquet
        .with_args(["install", "--ignore-scripts"])
        .assert()
        .success();
    let installed: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join("node_modules/custom-git/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(installed["name"], "custom-git");
    assert!(workspace.join("node_modules/custom-git/index.js").is_file());
    assert!(!workspace.join("node_modules/custom-git/excluded.txt").exists());
    pacquet_at(&workspace).with_args(["exec", "node", "-e",
        "console.log(require(require.resolve('@pnpm.e2e/dep-of-pkg-with-1-dep/package.json', { paths: [require.resolve('custom-git/package.json')] })).version)"])
        .assert().success().stdout("100.1.0\n");
    drop((root, mock_instance));
}
