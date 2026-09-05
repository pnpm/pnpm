use super::{LockedPackage, LockedWheel, PythonSpike, exact_requirement, wheel_name};
use crate::ecosystem_install::{
    EcosystemInstallCoordinator, EcosystemManifest, EcosystemWorkspaceInventory, MetadataMutation,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use pnpm_network::{AuthHeaders, ThrottledClient};
use pnpm_store_dir::{StoreDir, StoreIndexWriter};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    io::{Cursor, Read, Write},
    path::Path,
    sync::Arc,
};
use tokio::sync::Notify;
use zip::{ZipWriter, write::SimpleFileOptions};

fn wheel(name: &str, dependencies: &[&str], extra: &[(&str, &str)]) -> Vec<u8> {
    let dist_info = format!("{name}-1.0.dist-info");
    let mut metadata = format!("Metadata-Version: 2.4\nName: {name}\nVersion: 1.0\n");
    for dependency in dependencies {
        writeln!(metadata, "Requires-Dist: {dependency}").unwrap();
    }
    let mut files = vec![
        (format!("{name}/__init__.py"), format!("VALUE = '{name}'\n")),
        (format!("{dist_info}/METADATA"), metadata),
        (
            format!("{dist_info}/WHEEL"),
            "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n".into(),
        ),
    ];
    files.extend(extra.iter().map(|(path, contents)| (path.to_string(), contents.to_string())));
    let mut record = files.iter().fold(String::new(), |mut record, (path, contents)| {
        writeln!(
            record,
            "{path},sha256={},{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(contents)),
            contents.len(),
        )
        .unwrap();
        record
    });
    writeln!(record, "{dist_info}/RECORD,,").unwrap();
    files.push((format!("{dist_info}/RECORD"), record));
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in files {
        archive.start_file(path, SimpleFileOptions::default()).unwrap();
        archive.write_all(contents.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

async fn serve_wheel(
    server: &mut mockito::ServerGuard,
    name: &str,
    archive: Vec<u8>,
) -> (mockito::Mock, mockito::Mock) {
    let filename = wheel_name(name, "1.0");
    let index = server
        .mock("GET", format!("/simple/{name}/").as_str())
        .match_header("authorization", "Basic fixture-credential")
        .with_header("content-type", "application/vnd.pypi.simple.v1+json")
        .with_body(
            json!({
                "meta": { "api-version": "1.0" },
                "name": name,
                "files": [{
                    "filename": filename,
                    "url": format!("../../files/{filename}"),
                    "hashes": { "sha256": format!("{:x}", Sha256::digest(&archive)) },
                    "yanked": false,
                }],
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let artifact = server
        .mock("GET", format!("/files/{filename}").as_str())
        .match_header("authorization", "Basic fixture-credential")
        .with_body(archive)
        .expect(1)
        .create_async()
        .await;
    (index, artifact)
}

fn project(root: &Path, dependencies: &[&str]) -> EcosystemWorkspaceInventory {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("pyproject.toml"),
        format!(
            "[project]\nname = 'application'\nversion = '1.0'\ndependencies = {dependencies:?}\n",
        ),
    )
    .unwrap();
    EcosystemWorkspaceInventory::new(root.to_path_buf())
}

fn store(root: &Path) -> &'static StoreDir {
    let store = Box::leak(Box::new(StoreDir::from(root.to_path_buf())));
    store.init().unwrap();
    store
}

fn auth(server: &mockito::ServerGuard) -> AuthHeaders {
    let mut auth = AuthHeaders::default();
    auth.insert_url_header(&server.url(), "Basic fixture-credential".into());
    auth
}

#[tokio::test]
async fn resolves_transitive_wheels_and_restores_offline_through_the_shared_store() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let inventory = project(&root, &["Alpha==1.0"]);
    fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
    let mut server = mockito::Server::new_async().await;
    let alpha = serve_wheel(&mut server, "alpha", wheel("alpha", &["beta==1.0"], &[])).await;
    let beta = serve_wheel(&mut server, "beta", wheel("beta", &["alpha==1.0"], &[])).await;
    let client = ThrottledClient::default();
    let auth = auth(&server);
    let store = store(&temp.path().join("store"));
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let spike = PythonSpike {
        http_client: &client,
        auth_headers: &auth,
        store_dir: store,
        store_index_writer: writer,
        index_url: format!("{}/simple/", server.url()).parse().unwrap(),
        offline: false,
    };
    let plan = spike.resolve(&inventory).await.unwrap();
    assert_eq!(
        inventory.manifests(EcosystemManifest::Cargo).await.unwrap(),
        [root.join("Cargo.toml")],
    );
    assert_eq!(
        plan.lock.packages.iter().map(|package| package.name.as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"],
    );
    assert!(!plan.files.contains_key("package.json"), "raw wheels must not gain npm metadata");
    let output = temp.path().join("installed");
    fs::create_dir(&output).unwrap();
    let mutation =
        MetadataMutation::capture(output.clone(), plan.metadata_paths(&output)).await.unwrap();
    mutation.finish(plan.write(&output).await).unwrap();
    assert_eq!(
        fs::read_to_string(output.join("site-packages/alpha/__init__.py")).unwrap(),
        "VALUE = 'alpha'\n",
    );
    let lock = fs::read_to_string(output.join("pylock.toml")).unwrap();
    drop(spike);
    writer_task.await.unwrap().unwrap();
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let spike = PythonSpike {
        http_client: &client,
        auth_headers: &auth,
        store_dir: store,
        store_index_writer: writer,
        index_url: format!("{}/simple/", server.url()).parse().unwrap(),
        offline: true,
    };
    let restored = spike.restore(&lock).await.unwrap();
    assert_eq!(restored.files, plan.files);
    assert_eq!(toml::to_string(&restored.lock).unwrap(), lock);
    let error = spike.resolve(&inventory).await.err().expect("offline fresh resolution fails");
    assert!(error.to_string().contains("requires a lockfile"), "{error:?}");
    drop(spike);
    writer_task.await.unwrap().unwrap();
    alpha.0.assert_async().await;
    alpha.1.assert_async().await;
    beta.0.assert_async().await;
    beta.1.assert_async().await;
}

#[tokio::test]
async fn settles_python_writes_before_rolling_back_all_ecosystem_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let inventory = project(&root, &["alpha==1.0"]);
    let mut server = mockito::Server::new_async().await;
    let mocks = serve_wheel(&mut server, "alpha", wheel("alpha", &[], &[])).await;
    let client = ThrottledClient::default();
    let auth = auth(&server);
    let store = store(&temp.path().join("store"));
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let spike = PythonSpike {
        http_client: &client,
        auth_headers: &auth,
        store_dir: store,
        store_index_writer: writer,
        index_url: format!("{}/simple/", server.url()).parse().unwrap(),
        offline: false,
    };
    let plan = spike.resolve(&inventory).await.unwrap();
    fs::write(root.join("pylock.toml"), "old python lock").unwrap();
    fs::write(root.join("Cargo.lock"), "old cargo lock").unwrap();
    fs::write(root.join("pnpm-lock.yaml"), "old npm lock").unwrap();
    let mutation = MetadataMutation::capture(
        root.clone(),
        plan.metadata_paths(&root)
            .into_iter()
            .chain([root.join("Cargo.lock"), root.join("pnpm-lock.yaml")]),
    )
    .await
    .unwrap();
    let python_written = Arc::new(Notify::new());
    let outcome = EcosystemInstallCoordinator::new(async {
        python_written.notified().await;
        fs::write(root.join("pnpm-lock.yaml"), "new npm lock").unwrap();
        Err(miette::miette!("npm fixture failure"))
    })
    .with_install(async {
        fs::write(root.join("Cargo.lock"), "new cargo lock").unwrap();
        Ok(())
    })
    .with_install(async {
        plan.write(&root).await?;
        python_written.notify_one();
        Ok(())
    })
    .run_to_settlement()
    .await;
    let error = mutation.finish(outcome).unwrap_err();
    assert!(error.to_string().contains("npm fixture failure"), "{error:?}");
    assert_eq!(fs::read_to_string(root.join("pylock.toml")).unwrap(), "old python lock");
    assert_eq!(fs::read_to_string(root.join("Cargo.lock")).unwrap(), "old cargo lock");
    assert_eq!(fs::read_to_string(root.join("pnpm-lock.yaml")).unwrap(), "old npm lock");
    assert!(
        !root.join("site-packages/alpha/__init__.py").exists(),
        "new projection files must be rolled back",
    );
    assert!(
        plan.files.values().all(|path| path.exists()),
        "immutable CAS content survives rollback",
    );
    drop(spike);
    writer_task.await.unwrap().unwrap();
    mocks.0.assert_async().await;
    mocks.1.assert_async().await;
}

#[tokio::test]
async fn rejects_conflicting_transitive_pins_before_workspace_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let inventory = project(&root, &["alpha==1.0", "beta==1.0"]);
    let mut server = mockito::Server::new_async().await;
    let alpha = serve_wheel(&mut server, "alpha", wheel("alpha", &["beta==2.0"], &[])).await;
    let beta = serve_wheel(&mut server, "beta", wheel("beta", &[], &[])).await;
    let client = ThrottledClient::default();
    let auth = auth(&server);
    let store = store(&temp.path().join("store"));
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let spike = PythonSpike {
        http_client: &client,
        auth_headers: &auth,
        store_dir: store,
        store_index_writer: writer,
        index_url: format!("{}/simple/", server.url()).parse().unwrap(),
        offline: false,
    };
    let error = spike.resolve(&inventory).await.err().expect("conflict must fail");
    assert!(error.to_string().contains("conflicting Python pins"), "{error:?}");
    assert!(!root.join("pylock.toml").exists(), "resolution must not write the lock");
    drop(spike);
    writer_task.await.unwrap().unwrap();
    alpha.0.assert_async().await;
    alpha.1.assert_async().await;
    beta.0.assert_async().await;
    beta.1.assert_async().await;
}

#[test]
fn refuses_requirements_outside_the_spike_subset() {
    assert_eq!(exact_requirement(" Foo._-Bar == 1.0 ").unwrap(), ("foo-bar".into(), "1.0".into()));
    for requirement in [
        "alpha>=1",
        "alpha[extra]==1",
        "alpha==1; python_version < '3.12'",
        "alpha==1.*",
        "alpha==1.0rc1",
        "alpha==01",
        "../alpha==1",
        "alpha @ https://example.org/a.whl",
    ] {
        assert!(exact_requirement(requirement).is_err(), "must reject {requirement}");
    }
}

#[tokio::test]
async fn validates_wheel_records_metadata_and_supported_layouts() {
    let temp = tempfile::tempdir().unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(wheel("alpha", &[], &[]))).unwrap();
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let path = temp.path().join(index.to_string());
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        fs::write(&path, bytes).unwrap();
        files.insert(entry.name().to_string(), path);
    }
    let package = LockedPackage { name: "alpha".into(), version: "1.0".into(), wheels: vec![] };
    assert_eq!(super::wheel::validate(&package, &files).await.unwrap(), Vec::<String>::new());
    let cases = [
        ("alpha/__init__.py", "corrupt bytes", "RECORD verification failed"),
        (
            "alpha-1.0.dist-info/WHEEL",
            "Wheel-Version: 2.0\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
            "version 1.0 purelib",
        ),
        (
            "alpha-1.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: py3-none-any\n",
            "version 1.0 purelib",
        ),
        (
            "alpha-1.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: cp312-none-any\n",
            "version 1.0 purelib",
        ),
        (
            "alpha-1.0.dist-info/METADATA",
            "Metadata-Version: 2.4\nName: other\nVersion: 1.0\n",
            "identity mismatch",
        ),
        (
            "alpha-1.0.dist-info/METADATA",
            "Metadata-Version: 2.4\nName: alpha\nVersion: 1.0\nrequires-python: >=3.10\n",
            "unsupported Python spike wheel metadata",
        ),
        (
            "alpha-1.0.dist-info/RECORD",
            "alpha-1.0.dist-info/RECORD,,\n",
            "does not cover every wheel file",
        ),
        (
            "alpha-1.0.dist-info/RECORD",
            "alpha-1.0.dist-info/RECORD,,\nalpha-1.0.dist-info/RECORD,,\n",
            "duplicate Python RECORD entry",
        ),
    ];
    for (path, contents, expected) in cases {
        let source = &files[path];
        let original = fs::read(source).unwrap();
        fs::write(source, contents).unwrap();
        let error = super::wheel::validate(&package, &files).await.unwrap_err();
        assert!(error.to_string().contains(expected), "{path}: {error:?}");
        fs::write(source, original).unwrap();
    }
    for path in [
        "alpha-1.0.data/scripts/hello",
        "alpha-1.0.dist-info/entry_points.txt",
        "other-1.0.dist-info/METADATA",
    ] {
        files.insert(path.into(), files["alpha/__init__.py"].clone());
        let error = super::wheel::validate(&package, &files).await.unwrap_err();
        assert!(error.to_string().contains("unsupported Python spike wheel path"), "{error:?}");
        files.remove(path);
    }
}

#[tokio::test]
async fn verifies_archive_integrity_and_refuses_an_offline_cache_miss() {
    let temp = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let archive = wheel("alpha", &[], &[]);
    let artifact = server
        .mock("GET", "/alpha.whl")
        .with_body("tampered archive")
        .expect(1)
        .create_async()
        .await;
    let client = ThrottledClient::default();
    let auth = AuthHeaders::default();
    let store = store(&temp.path().join("store"));
    let (writer, writer_task) = StoreIndexWriter::spawn(store);
    let mut spike = PythonSpike {
        http_client: &client,
        auth_headers: &auth,
        store_dir: store,
        store_index_writer: writer,
        index_url: format!("{}/simple/", server.url()).parse().unwrap(),
        offline: false,
    };
    let package = LockedPackage {
        name: "alpha".into(),
        version: "1.0".into(),
        wheels: vec![LockedWheel {
            name: wheel_name("alpha", "1.0"),
            url: format!("{}/alpha.whl", server.url()),
            hashes: BTreeMap::from([("sha256".into(), format!("{:x}", Sha256::digest(&archive)))]),
        }],
    };
    let error = spike.ingest(&package).await.unwrap_err();
    assert!(format!("{error:?}").contains("integrity"), "{error:?}");
    spike.offline = true;
    let error = spike.ingest(&package).await.unwrap_err();
    assert!(error.to_string().contains("offline"), "{error:?}");
    drop(spike);
    writer_task.await.unwrap().unwrap();
    artifact.assert_async().await;
}

#[test]
fn refuses_two_wheels_installing_the_same_path() {
    let mut files = BTreeMap::from([("shared.py".into(), "first-cas-file".into())]);
    let error = super::merge_files(
        &mut files,
        BTreeMap::from([("shared.py".into(), "second-cas-file".into())]),
    )
    .unwrap_err();
    assert!(error.to_string().contains("same path: shared.py"), "{error:?}");
}
