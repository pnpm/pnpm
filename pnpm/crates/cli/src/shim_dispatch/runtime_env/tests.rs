use super::{
    MAX_RUNTIME_LAUNCHER_SIZE, RUNTIME_LAUNCHER_SCHEMA, RuntimeLauncher, cached_runtime_bin_at,
    runtime_environment_key, runtime_launcher_key, runtime_launcher_path, write_runtime_launcher,
};
use std::{fs, path::PathBuf};

struct RuntimeFixture {
    root: tempfile::TempDir,
    environments_dir: PathBuf,
    store_dir: PathBuf,
    environment_key: String,
    executable: PathBuf,
}

impl RuntimeFixture {
    fn new(name: &str, version_spec: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let environments_dir = root.path().join("state/global-shim-runtimes");
        let store_dir = root.path().join("store/links");
        let package_dir = store_dir.join("runtime/slot/node_modules").join(name);
        let environment_key = runtime_environment_key(name, version_spec, &store_dir);
        let environment_modules = environments_dir.join(&environment_key).join("node_modules");
        fs::create_dir_all(package_dir.join("bin")).unwrap();
        fs::create_dir_all(&environment_modules).unwrap();
        fs::write(
            package_dir.join("package.json"),
            serde_json::json!({ "name": name, "bin": { (name): format!("bin/{name}") } })
                .to_string(),
        )
        .unwrap();
        let executable = package_dir.join("bin").join(name);
        fs::write(&executable, "runtime").unwrap();
        pacquet_fs::symlink_dir(&package_dir, &environment_modules.join(name)).unwrap();
        Self { root, environments_dir, store_dir, environment_key, executable }
    }

    fn write_launcher(&self, name: &str, version_spec: &str) {
        write_runtime_launcher(
            &self.environments_dir,
            name,
            version_spec,
            &self.environment_key,
            &self.store_dir,
        )
        .unwrap();
    }

    fn launcher_path(&self, name: &str, version_spec: &str) -> PathBuf {
        runtime_launcher_path(&self.environments_dir, &runtime_launcher_key(name, version_spec))
    }
}

#[test]
fn warm_launcher_revalidates_the_runtime_in_the_store() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");

    assert_eq!(
        cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"),
        Some(fs::canonicalize(fixture.executable).unwrap()),
    );
}

#[test]
fn launcher_key_binds_the_exact_runtime_and_host() {
    assert_ne!(runtime_launcher_key("node", "22"), runtime_launcher_key("node", "22.0.0"));
    assert_ne!(runtime_launcher_key("node", "22"), runtime_launcher_key("deno", "22"));
}

#[test]
fn mismatched_launcher_provenance_is_a_cache_miss() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    let requested_key = runtime_launcher_key("node", "22.11.0");
    let launcher = RuntimeLauncher {
        schema: RUNTIME_LAUNCHER_SCHEMA,
        launcher_key: runtime_launcher_key("node", "20.0.0"),
        environment_key: fixture.environment_key.clone(),
        global_virtual_store_dir: fixture.store_dir.clone(),
    };
    let path = runtime_launcher_path(&fixture.environments_dir, &requested_key);
    pacquet_fs::write_atomic(&path, &serde_json::to_vec(&launcher).unwrap()).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[test]
fn launcher_cannot_redirect_the_environment_key() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    let launcher_key = runtime_launcher_key("node", "22.11.0");
    let launcher = RuntimeLauncher {
        schema: RUNTIME_LAUNCHER_SCHEMA,
        launcher_key: launcher_key.clone(),
        environment_key: "a".repeat(64),
        global_virtual_store_dir: fixture.store_dir.clone(),
    };
    let path = runtime_launcher_path(&fixture.environments_dir, &launcher_key);
    pacquet_fs::write_atomic(&path, &serde_json::to_vec(&launcher).unwrap()).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[test]
fn launcher_cannot_widen_the_store_with_a_relative_path() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    let launcher_key = runtime_launcher_key("node", "22.11.0");
    let launcher = RuntimeLauncher {
        schema: RUNTIME_LAUNCHER_SCHEMA,
        launcher_key: launcher_key.clone(),
        environment_key: fixture.environment_key.clone(),
        global_virtual_store_dir: PathBuf::from("."),
    };
    let path = runtime_launcher_path(&fixture.environments_dir, &launcher_key);
    pacquet_fs::write_atomic(&path, &serde_json::to_vec(&launcher).unwrap()).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[test]
fn corrupt_or_oversized_launchers_are_cache_misses() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    let path = fixture.launcher_path("node", "22.11.0");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "not json").unwrap();
    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);

    fs::write(path, vec![b'x'; MAX_RUNTIME_LAUNCHER_SIZE as usize + 1]).unwrap();
    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[cfg(unix)]
#[test]
fn symlinked_launcher_is_never_followed() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");
    let path = fixture.launcher_path("node", "22.11.0");
    let contents = fs::read(&path).unwrap();
    let outside = fixture.root.path().join("outside.json");
    fs::write(&outside, contents).unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside, path).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[cfg(unix)]
#[test]
fn launcher_never_executes_a_package_outside_its_store() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");
    let package_link =
        fixture.environments_dir.join(&fixture.environment_key).join("node_modules/node");
    fs::remove_file(&package_link).unwrap();
    let outside = fixture.root.path().join("outside/node");
    fs::create_dir_all(outside.join("bin")).unwrap();
    fs::write(outside.join("package.json"), r#"{"name":"node","bin":{"node":"bin/node"}}"#)
        .unwrap();
    fs::write(outside.join("bin/node"), "compromised").unwrap();
    std::os::unix::fs::symlink(outside, package_link).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[cfg(unix)]
#[test]
fn launcher_never_executes_a_manifest_bin_outside_its_package() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");
    fs::remove_file(&fixture.executable).unwrap();
    let outside = fixture.store_dir.join("outside-node");
    fs::write(&outside, "compromised").unwrap();
    std::os::unix::fs::symlink(outside, &fixture.executable).unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[test]
fn launcher_rechecks_the_provider_name() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");
    let package = fixture.executable.parent().unwrap().parent().unwrap();
    fs::write(package.join("package.json"), r#"{"name":"lookalike","bin":{"node":"bin/node"}}"#)
        .unwrap();

    assert_eq!(cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0"), None);
}

#[test]
fn launcher_writer_rejects_unbound_environment_keys() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    let error = write_runtime_launcher(
        &fixture.environments_dir,
        "node",
        "22.11.0",
        &"f".repeat(64),
        &fixture.store_dir,
    )
    .unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn concurrent_launcher_refreshes_never_publish_partial_json() {
    let fixture = RuntimeFixture::new("node", "22.11.0");
    fixture.write_launcher("node", "22.11.0");

    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..20 {
                    fixture.write_launcher("node", "22.11.0");
                }
            });
        }
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..100 {
                    assert!(
                        cached_runtime_bin_at(&fixture.environments_dir, "node", "22.11.0",)
                            .is_some(),
                    );
                }
            });
        }
    });
}
