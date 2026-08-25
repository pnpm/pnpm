//! Single-project `pack` integration tests, covering the `beforePacking`
//! pnpmfile hook and the settings that suppress the pack lifecycle
//! scripts. For the hook: the published manifest a package is
//! packed with must reflect what the hook returns, exactly as pnpm's
//! `pnpm pack` / `pnpm publish` apply it. This is the mechanism the pnpm
//! CLI's own release relies on to strip its bundled dependency fields
//! from the published manifest (pnpm/pnpm#12955).

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::{fs, path::Path};

#[test]
fn pack_uses_embed_readme_and_manifest_obfuscation_settings() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "scripts": { "prepublishOnly": "echo kept" },
        })
        .to_string(),
    )
    .unwrap();
    fs::write(workspace.join("README.md"), "# Packed README\n").unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "embedReadme: true\nskipManifestObfuscation: true\n",
    )
    .unwrap();

    pacquet.with_arg("pack").assert().success();

    let manifest = read_manifest_from_tarball(&workspace.join("pkg-1.0.0.tgz"));
    assert_eq!(manifest["readme"], "# Packed README\n");
    assert_eq!(manifest["scripts"]["prepublishOnly"], "echo kept");

    drop(root);
}

/// A `beforePacking` hook that deletes `devDependencies` and stamps a
/// marker rewrites the manifest packed into the tarball.
#[test]
fn before_packing_hook_rewrites_the_packed_manifest() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "devDependencies": { "internal-only": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      delete manifest.devDependencies
      manifest.packedByHook = true
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    let out = workspace.join("tarballs");
    fs::create_dir_all(&out).expect("create out dir");

    pacquet
        .with_arg("pack")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();

    let manifest = read_manifest_from_tarball(&out.join("pkg-1.0.0.tgz"));
    assert_eq!(manifest["packedByHook"], json!(true), "the hook's added field must be packed");
    assert!(
        manifest.get("devDependencies").is_none(),
        "the field the hook deleted must not be in the packed manifest",
    );

    drop(root);
}

/// The workspace-root `.pnpmfile.cjs` applies to a `--filter`-selected
/// sub-package, the exact shape pnpm's `release.yml` drives with
/// `pn publish --filter=<pkg>`: the pnpmfile lives at the repo root while
/// the packed project is a nested workspace package.
#[test]
fn workspace_root_before_packing_hook_applies_to_a_filtered_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      if (manifest.name === 'pkg') {
        delete manifest.dependencies
      }
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    let pkg_dir = workspace.join("packages/pkg");
    fs::create_dir_all(&pkg_dir).expect("create package dir");
    fs::write(
        pkg_dir.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "dependencies": { "left-pad": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    let out = workspace.join("tarballs");
    fs::create_dir_all(&out).expect("create out dir");

    pacquet
        .with_arg("--filter")
        .with_arg("pkg")
        .with_arg("pack")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();

    let manifest = read_manifest_from_tarball(&out.join("pkg-1.0.0.tgz"));
    assert!(
        manifest.get("dependencies").is_none(),
        "the workspace-root pnpmfile's beforePacking hook must strip the sub-package's dependencies",
    );

    drop(root);
}

/// A recursive pack applies the workspace-root `beforePacking` hook to
/// every packed project. The hooks are loaded once and shared across
/// projects, so this also exercises the shared-worker path.
#[test]
fn recursive_pack_applies_before_packing_hook_to_every_project() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = {
  hooks: {
    beforePacking (manifest) {
      manifest.packedByHook = true
      return manifest
    },
  },
}
",
    )
    .expect("write .pnpmfile.cjs");
    for name in ["project-1", "project-2"] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create package dir");
        fs::write(
            dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0" }).to_string(),
        )
        .expect("write package.json");
    }
    let out = workspace.join("tarballs");
    fs::create_dir_all(&out).expect("create out dir");

    pacquet
        .with_arg("-r")
        .with_arg("pack")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();

    for name in ["project-1", "project-2"] {
        let manifest = read_manifest_from_tarball(&out.join(format!("{name}-1.0.0.tgz")));
        assert_eq!(
            manifest["packedByHook"],
            json!(true),
            "the workspace-root beforePacking hook must run for {name}",
        );
    }

    drop(root);
}

/// `--config.ignore-scripts=true` suppresses the pack lifecycle scripts
/// just like the bare `--ignore-scripts` flag does for the commands that
/// declare it. `pack` has no such flag, so the dotted override is the only
/// way to skip its `prepack` (pnpm/pnpm#13986).
#[test]
fn config_ignore_scripts_override_suppresses_prepack() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({
            "name": "pkg",
            "version": "1.0.0",
            "scripts": {
                "prepack": r#"node -e "require('fs').writeFileSync('prepack-ran.txt','ran')""#,
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    let out = workspace.join("tarballs");
    fs::create_dir_all(&out).expect("create out dir");
    let marker = workspace.join("prepack-ran.txt");

    pacquet
        .with_arg("pack")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();
    assert!(marker.exists(), "prepack must run when nothing suppresses it");
    let tarball = out.join("pkg-1.0.0.tgz");
    fs::remove_file(&marker).expect("remove marker");
    fs::remove_file(&tarball).expect("remove the first tarball");

    let CommandTempCwd { pacquet: ignoring, root: ignoring_root, .. } = CommandTempCwd::init();
    ignoring
        .with_current_dir(&workspace)
        .with_arg("pack")
        .with_arg("--config.ignore-scripts=true")
        .with_arg("--pack-destination")
        .with_arg(out.to_str().expect("utf8 out dir"))
        .assert()
        .success();
    assert!(!marker.exists(), "--config.ignore-scripts=true must suppress prepack");
    assert!(tarball.exists(), "the tarball must still be packed");

    drop((root, ignoring_root));
}

/// Extract `package/package.json` from a packed tarball.
fn read_manifest_from_tarball(tarball: &Path) -> serde_json::Value {
    use std::io::Read as _;

    let bytes = fs::read(tarball).expect("read tarball");
    let decoder = flate2::read::GzDecoder::new(bytes.as_slice());
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().expect("iterate tarball entries") {
        let mut entry = entry.expect("read tarball entry");
        if entry.path().expect("entry path") == Path::new("package/package.json") {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).expect("read manifest");
            return serde_json::from_str(&contents).expect("parse manifest");
        }
    }
    panic!("package/package.json not found in {}", tarball.display());
}
