use super::{mock_wheel_downloads, pacquet_in, project, serve, wheel};
use assert_cmd::prelude::*;
use std::{
    fs,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

fn member(root: &std::path::Path, name: &str, dependencies: &[&str]) {
    let path = root.join(name);
    fs::create_dir(&path).unwrap();
    fs::write(path.join("pyproject.toml"), format!(
        "[project]\nname = '{name}-app'\nversion = '1.0'\nrequires-python = '>=3.10'\ndependencies = {dependencies:?}\n",
    )).unwrap();
}

#[tokio::test]
async fn identical_projects_share_one_fresh_registry_resolution() {
    for lockfile_only in [true, false] {
        eprintln!("lockfile_only={lockfile_only}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let mut requests =
            serve(&mut server, "demo", &[("1.0", wheel("demo", "1.0", "", &[]))]).await;
        let index = requests.pop().unwrap();
        let index = index.expect(1);
        project(root.path(), &server.url(), &["demo"]);
        for name in ["one", "two", "three"] {
            member(root.path(), name, &["demo"]);
        }
        let mut command = pacquet_in(root.path());
        command.env("PNPM_CONFIG_WORKSPACE_CONCURRENCY", "3").arg("install");
        if lockfile_only {
            command.arg("--lockfile-only");
        }
        command.assert().success();
        let expected = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        for name in ["one", "two", "three"] {
            assert_eq!(
                fs::read_to_string(
                    root.path()
                        .join(name)
                        .join("pylock.toml")
                )
                .unwrap(),
                expected,
            );
            assert_eq!(
                root.path()
                    .join(name)
                    .join(".venv")
                    .exists(),
                !lockfile_only,
            );
        }
        index.assert_async().await;
        pacquet_in(root.path())
            .args(["install", "--lockfile-only", "--frozen-lockfile"])
            .assert()
            .success();
        fs::remove_file(root.path().join("two/pylock.toml")).unwrap();
        pacquet_in(root.path())
            .args(["install", "--lockfile-only", "--frozen-lockfile"])
            .assert()
            .failure();
    }
}

#[tokio::test]
async fn project_preparation_replenishes_concurrent_slots() {
    for fail in [false, true] {
        eprintln!("fail={fail}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archives = [
            ("alpha", wheel("alpha", "1.0", "", &[])),
            ("beta", wheel("beta", "1.0", "", &[])),
            ("gamma", wheel("gamma", "1.0", "", &[])),
        ];
        let _indexes = serve_indexes(&mut server, &archives).await;
        project(root.path(), &server.url(), &["gamma"]);
        member(root.path(), "alpha", &["alpha"]);
        member(root.path(), "beta", &["beta"]);
        let rendezvous = Arc::new((Mutex::new(0), Condvar::new()));
        let finished = Arc::new(AtomicBool::new(false));
        let downloads =
            mock_wheel_downloads(&mut server, archives, fail, &rendezvous, &finished).await;
        let mut command = pacquet_in(root.path());
        command
            .env("PNPM_CONFIG_WORKSPACE_CONCURRENCY", "2")
            .args(["install", "--lockfile-only"]);
        if fail {
            command.assert().failure();
            assert!(!root.path().join("pylock.toml").exists(), "published a failed plan");
        } else {
            command.assert().success();
        }
        assert!(finished.load(Ordering::SeqCst), "returned before sibling finished");
        for request in downloads {
            request.assert_async().await;
        }
        assert_eq!(*rendezvous.0.lock().unwrap(), 3);
    }
}

#[tokio::test]
async fn different_resolution_inputs_do_not_share_a_fresh_lockfile() {
    for change_python_range in [false, true] {
        eprintln!("change_python_range={change_python_range}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let mut requests =
            serve(&mut server, "demo", &[("1.0", wheel("demo", "1.0", "", &[]))]).await;
        let index = requests.pop().unwrap().expect(2);
        project(root.path(), &server.url(), &["demo"]);
        member(root.path(), "other", &[if change_python_range { "demo" } else { "demo==1.0" }]);
        let manifest = root.path().join("other/pyproject.toml");
        if change_python_range {
            let contents = fs::read_to_string(&manifest).unwrap();
            fs::write(&manifest, contents.replace(">=3.10", ">=3.9")).unwrap();
        }
        pacquet_in(root.path())
            .args(["install", "--lockfile-only"])
            .assert()
            .success();
        let first = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        let second = fs::read_to_string(root.path().join("other/pylock.toml")).unwrap();
        eprintln!("FIRST:\n{first}\nSECOND:\n{second}");
        assert_ne!(first, second);
        index.assert_async().await;
    }
}

async fn serve_indexes(
    server: &mut mockito::ServerGuard,
    archives: &[(&str, Vec<u8>)],
) -> Vec<mockito::Mock> {
    let mut indexes = Vec::new();
    for (name, archive) in archives {
        let mut requests = serve(server, name, &[("1.0", archive.clone())]).await;
        indexes.push(requests.pop().unwrap());
        for request in requests {
            request.remove_async().await;
        }
    }
    indexes
}
