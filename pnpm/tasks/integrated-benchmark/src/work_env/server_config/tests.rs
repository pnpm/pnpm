use super::{
    cold_mock_config_yaml, cold_mock_registries_yaml, pnpr_benchmark_config_yaml,
    pnpr_benchmark_public_url, pnpr_benchmark_registries_yaml, write_pnpr_benchmark_config,
};
use crate::work_env::{
    registries_home::{RegistriesHome, fixture_registries, revision_source},
    revision_mock::pnpr_server_command,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn parse(yaml: &str) -> Value {
    serde_saphyr::from_str(yaml).unwrap()
}

/// A checkout at `source` with the registry-mock fixtures of a revision whose
/// registries live where `registries` says.
fn revision_checkout(source: &Path, registries: RegistriesHome) {
    fs::create_dir_all(source.join("pnpr/.fixtures")).unwrap();
    fs::write(source.join("pnpr/.fixtures/config.yaml"), "auth:\n  users: {}\n").unwrap();
    if registries == RegistriesHome::Database {
        fs::write(source.join("pnpr/.fixtures/registries.yaml"), "registries: {}\n").unwrap();
    }
}

#[test]
fn a_revision_with_a_fixture_registries_document_reads_them_from_the_database() {
    let directory = tempfile::tempdir().unwrap();
    for registries in [RegistriesHome::ConfigFile, RegistriesHome::Database] {
        let source = directory
            .path()
            .join(format!("{registries:?}"));
        revision_checkout(&source, registries);
        assert_eq!(RegistriesHome::of_source(&source), registries);
        let binary = source.join("target/release/pnpr");
        assert_eq!(revision_source(&binary), source);
        assert_eq!(fixture_registries(&source).is_some(), registries == RegistriesHome::Database);
    }
}

#[test]
fn database_cold_mock_imports_its_upstream_and_allows_loopback() {
    let storage = Path::new("cold-mock-storage");
    let config =
        parse(&cold_mock_config_yaml(storage, "http://127.0.0.1:4874/", RegistriesHome::Database));
    assert!(config.get("registries").is_none());
    assert!(config.get("defaultRegistry").is_none());
    assert_eq!(config["routes"], json!({"allowedPrivateNetworks": ["127.0.0.0/8", "::1"]}));
    assert_eq!(config["storage"], "cold-mock-storage");

    let document = parse(&cold_mock_registries_yaml("http://127.0.0.1:4874/"));
    assert_eq!(
        document,
        json!({
            "registries": {
                "npmjs": {"type": "upstream", "url": "http://127.0.0.1:4874/", "public": true},
            },
            "defaultRegistry": "npmjs",
        }),
    );
}

#[test]
fn config_file_cold_mock_keeps_its_upstream_in_the_config() {
    let config = parse(&cold_mock_config_yaml(
        Path::new("cold-mock-storage"),
        "http://127.0.0.1:4874/",
        RegistriesHome::ConfigFile,
    ));
    assert_eq!(config["registries"]["npmjs"]["url"], "http://127.0.0.1:4874/");
    assert_eq!(config["defaultRegistry"], "npmjs");
    assert!(config.get("routes").is_none());
}

#[test]
fn database_pnpr_server_imports_only_its_public_routes() {
    let routes = ["http://localhost:4873/", "http://127.0.0.1:61824/"];
    let config = parse(&pnpr_benchmark_config_yaml(
        Path::new("pnpr-storage"),
        &routes,
        json!({"users": {"maxUsers": 1}}),
        RegistriesHome::Database,
    ));
    assert_eq!(config["routes"], json!({"allowedPrivateNetworks": ["127.0.0.0/8", "::1"]}));
    assert_eq!(config["auth"], json!({"users": {"maxUsers": 1}}));

    let document = parse(&pnpr_benchmark_registries_yaml(&routes));
    assert_eq!(
        document,
        json!({
            "routes": {
                "public": [
                    {"registry": "http://localhost:4873/"},
                    {"registry": "http://127.0.0.1:61824/"},
                ],
            },
        }),
    );
}

#[test]
fn pnpr_server_files_follow_the_revision() {
    let directory = tempfile::tempdir().unwrap();
    for registries in [RegistriesHome::ConfigFile, RegistriesHome::Database] {
        let bench_dir = directory
            .path()
            .join(format!("{registries:?}"));
        revision_checkout(&bench_dir.join("pacquet"), registries);
        let storage = bench_dir.join("pnpr-storage");
        let files = write_pnpr_benchmark_config(&bench_dir, &storage, &["http://localhost:4873/"]);
        let config = parse(&fs::read_to_string(&files.config).unwrap());
        let document = files.registries.map(|path| parse(&fs::read_to_string(path).unwrap()));
        dbg!(registries, &config, &document);
        match registries {
            RegistriesHome::ConfigFile => {
                assert_eq!(config["routes"]["public"][0]["registry"], "http://localhost:4873/");
                assert!(document.is_none());
            }
            RegistriesHome::Database => {
                assert!(config["routes"].get("public").is_none());
                let document = document.unwrap();
                assert_eq!(document["routes"]["public"][0]["registry"], "http://localhost:4873/");
            }
        }
    }
}

#[test]
fn only_a_database_pnpr_server_gets_a_public_url_of_its_own_host() {
    let directory = tempfile::tempdir().unwrap();
    let registries = directory.path().join("pnpr-registries.yaml");
    for document in [None, Some(registries.as_path())] {
        let command = pnpr_server_command(&super::PnprServerPaths {
            bench_dir: directory.path(),
            binary: Path::new("pnpr"),
            config: Path::new("pnpr-config.yaml"),
            registries: document,
            storage: Path::new("pnpr-storage"),
            port: 4321,
        });
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect();
        let public_url = args
            .windows(2)
            .find(|pair| pair[0] == "--public-url")
            .map(|pair| pair[1]);
        dbg!(&args);
        match document {
            None => assert_eq!(public_url, None),
            Some(_) => assert_eq!(public_url, Some(pnpr_benchmark_public_url(4321).as_str())),
        }
    }
    let public_url = reqwest::Url::parse(&pnpr_benchmark_public_url(4321)).unwrap();
    assert_ne!(public_url.host_str(), Some("localhost"));
    assert_ne!(public_url.host_str(), Some("127.0.0.1"));
}

/// A stand-in `pnpr` that records each run's arguments, one per line.
#[cfg(unix)]
fn recording_binary(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let binary = dir.join("pnpr");
    fs::write(
        &binary,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", dir.join("record").display()),
    )
    .unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}

#[cfg(unix)]
#[test]
fn database_cold_mock_command_imports_its_registries_first() {
    let directory = tempfile::tempdir().unwrap();
    let binary = recording_binary(directory.path());
    let command = super::cold_revision_mock_command(
        &super::RevisionMockLaunch {
            revision: "HEAD",
            binary: &binary,
            bench_dir: directory.path(),
            port: 4873,
            public_url: "http://localhost:4872",
            registries: RegistriesHome::Database,
        },
        "http://127.0.0.1:4874/-/npm/",
    );
    let document = directory.path().join("cold-mock-registries.yaml");
    let mut import: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_str().unwrap().to_string())
        .collect();
    import.extend(["registries".to_string(), "import".to_string()]);
    import.push(document.to_str().unwrap().to_string());
    let record = fs::read_to_string(directory.path().join("record")).unwrap();
    assert_eq!(record.lines().collect::<Vec<_>>(), import);
    assert_eq!(
        parse(&fs::read_to_string(document).unwrap())["registries"]["npmjs"]["url"],
        "http://127.0.0.1:4874/-/npm/",
    );
}
