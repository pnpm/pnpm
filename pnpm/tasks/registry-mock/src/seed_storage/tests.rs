use super::{LinkOutcome, copy_into_new_file, seed_generation};
use pnpr_fixtures::FixtureGeneration;
use serde_json::Value;
use std::{fs, path::Path};
use tempfile::TempDir;

fn write_fixture(fixtures: &Path, name: &str, version: &str) {
    let version_dir = fixtures.join(name).join(version);
    fs::create_dir_all(&version_dir).expect("create fixture version dir");
    fs::write(
        version_dir.join("package.json"),
        serde_json::to_vec(&serde_json::json!({ "name": name, "version": version }))
            .expect("serialize fixture manifest"),
    )
    .expect("write fixture manifest");
}

/// A fixture generation of the test's own making, built through the same
/// `pnpr-fixtures` entry point the committed fixtures go through. The
/// fixture tree is gone by the time this returns: `of` publishes the
/// storage under `generated`, and the storage is what a generation reads.
fn generation(packages: &[(&str, &str)], generated: &Path) -> FixtureGeneration {
    let fixtures = TempDir::new().expect("create fixture tree");
    for (name, version) in packages {
        write_fixture(fixtures.path(), name, version);
    }
    FixtureGeneration::of(fixtures.path(), generated)
}

fn packument(storage: &Path, package: &str) -> Value {
    let path = storage.join(package).join("package.json");
    serde_json::from_slice(&fs::read(&path).expect("read packument")).expect("parse packument")
}

fn versions(storage: &Path, package: &str) -> Vec<String> {
    let mut versions: Vec<String> = packument(storage, package)["versions"]
        .as_object()
        .expect("packument versions object")
        .keys()
        .cloned()
        .collect();
    versions.sort();
    versions
}

fn description(storage: &Path, package: &str) -> Value {
    packument(storage, package)["versions"]["1.0.0"]["description"].clone()
}

#[test]
fn a_changed_fixture_set_is_not_served_from_the_previous_generations_directory() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let first = generation(&[("@e2e/kept", "1.0.0")], generated.path());
    let second = generation(&[("@e2e/kept", "1.0.0"), ("@e2e/kept", "2.0.0")], generated.path());

    let first_storage = seed_generation(&first, root.path()).expect("seed the first generation");
    assert_eq!(versions(first_storage.path(), "@e2e/kept"), ["1.0.0"]);

    let second_storage = seed_generation(&second, root.path()).expect("seed the second generation");
    assert_eq!(versions(second_storage.path(), "@e2e/kept"), ["1.0.0", "2.0.0"]);
    assert_ne!(
        first_storage.path(),
        second_storage.path(),
        "two generations must not share a runtime storage directory",
    );
}

#[test]
fn a_package_the_fixtures_dropped_is_absent_from_its_generations_directory() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let first = generation(&[("@e2e/dropped", "1.0.0")], generated.path());
    let second = generation(&[("@e2e/kept", "1.0.0")], generated.path());

    let first_storage = seed_generation(&first, root.path()).expect("seed the first generation");
    assert!(
        first_storage
            .path()
            .join("@e2e/dropped")
            .exists(),
    );

    let second_storage = seed_generation(&second, root.path()).expect("seed the second generation");
    assert!(
        !second_storage
            .path()
            .join("@e2e/dropped")
            .exists(),
    );
    assert!(
        second_storage
            .path()
            .join("@e2e/kept/package.json")
            .exists(),
    );
}

#[test]
fn concurrent_fixture_generations_are_seeded_into_separate_directories() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let first = generation(&[("@e2e/first", "1.0.0")], generated.path());
    let second = generation(&[("@e2e/second", "1.0.0")], generated.path());

    let (first_storage, second_storage) = std::thread::scope(|scope| {
        let first =
            scope.spawn(|| seed_generation(&first, root.path()).expect("seed first generation"));
        let second =
            scope.spawn(|| seed_generation(&second, root.path()).expect("seed second generation"));
        (first.join().expect("first seed thread"), second.join().expect("second seed thread"))
    });

    assert_ne!(first_storage.path(), second_storage.path());
    assert!(
        first_storage
            .path()
            .join("@e2e/first/package.json")
            .exists(),
    );
    assert!(
        !first_storage
            .path()
            .join("@e2e/second")
            .exists(),
    );
    assert!(
        second_storage
            .path()
            .join("@e2e/second/package.json")
            .exists(),
    );
    assert!(
        !second_storage
            .path()
            .join("@e2e/first")
            .exists(),
    );
}

#[test]
fn concurrent_launches_publish_one_complete_generation_marker() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let current = generation(&[("@e2e/shared", "1.0.0")], generated.path());

    let storages = std::thread::scope(|scope| {
        let workers = std::array::from_fn::<_, 8, _>(|_| {
            scope.spawn(|| seed_generation(&current, root.path()).expect("seed generation"))
        });
        workers.map(|worker| worker.join().expect("seed thread"))
    });

    assert!(
        storages
            .iter()
            .all(|storage| storage.path() == storages[0].path()),
    );
    assert!(
        storages
            .iter()
            .all(|storage| storage
                .path()
                .join("@e2e/shared/package.json")
                .exists()),
    );
    assert_eq!(
        fs::read_to_string(storages[0].path().join(".fixture-generation"))
            .expect("read published marker"),
        current.fingerprint(),
    );
}

#[test]
fn changed_fixture_metadata_reaches_the_seeded_packument() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");

    let fixtures = TempDir::new().expect("create fixture tree");
    write_fixture(fixtures.path(), "@e2e/described", "1.0.0");
    let first = FixtureGeneration::of(fixtures.path(), generated.path());
    let manifest = fixtures.path().join("@e2e/described/1.0.0/package.json");
    fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "name": "@e2e/described",
            "version": "1.0.0",
            "description": "second revision",
        }))
        .expect("serialize fixture manifest"),
    )
    .expect("rewrite fixture manifest");
    let second = FixtureGeneration::of(fixtures.path(), generated.path());

    let first_storage = seed_generation(&first, root.path()).expect("seed the first generation");
    assert!(description(first_storage.path(), "@e2e/described").is_null());

    let second_storage = seed_generation(&second, root.path()).expect("seed the second generation");
    assert_eq!(description(second_storage.path(), "@e2e/described"), "second revision");
}

#[test]
fn a_relaunch_of_one_generation_keeps_what_the_cache_holds() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let current = generation(&[("@e2e/kept", "1.0.0")], generated.path());

    let first = seed_generation(&current, root.path()).expect("seed the generation");
    assert!(first.seeded_files() > 0, "a fresh generation seeds its files");

    // What `pnpr` and a benchmark scenario write into the directory
    // between launches: neither is the generation's to remove.
    let proxy_entry = first.path().join("left-pad");
    fs::create_dir_all(&proxy_entry).expect("create proxy cache entry dir");
    fs::write(
        proxy_entry.join("package.json"),
        serde_json::to_vec(&serde_json::json!({ "name": "left-pad", "dist-tags": {} }))
            .expect("serialize proxy packument"),
    )
    .expect("write proxy packument");

    let second = seed_generation(&current, root.path()).expect("relaunch the same generation");
    assert_eq!(first.path(), second.path());
    assert_eq!(second.seeded_files(), 0, "an already-seeded generation re-seeds nothing");
    assert_eq!(versions(second.path(), "@e2e/kept"), ["1.0.0"]);
    assert!(
        proxy_entry.join("package.json").exists(),
        "relaunching a generation must not drop proxy-cache entries",
    );
}

#[test]
fn benchmark_added_packages_stay_with_their_fixture_generation() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let first = generation(&[("@e2e/kept", "1.0.0")], generated.path());
    let second = generation(&[("@e2e/kept", "2.0.0")], generated.path());

    let first_storage = seed_generation(&first, root.path()).expect("seed the first generation");
    let benchmark_package = "@pnpmtest/peer-benchmark-level-0-00";
    let benchmark_dir = first_storage.path().join(benchmark_package);
    fs::create_dir_all(&benchmark_dir).expect("create benchmark package directory");
    fs::write(
        benchmark_dir.join("package.json"),
        serde_json::to_vec(&serde_json::json!({ "name": benchmark_package, "versions": {} }))
            .expect("serialize benchmark packument"),
    )
    .expect("write benchmark packument");

    let second_storage = seed_generation(&second, root.path()).expect("seed the second generation");
    assert!(
        !second_storage
            .path()
            .join(benchmark_package)
            .exists(),
    );

    let relaunched = seed_generation(&first, root.path()).expect("relaunch the first generation");
    assert_eq!(relaunched.path(), first_storage.path());
    assert_eq!(
        fs::read(benchmark_dir.join("package.json")).expect("read benchmark packument"),
        serde_json::to_vec(&serde_json::json!({ "name": benchmark_package, "versions": {} }))
            .expect("serialize benchmark packument"),
    );
}

#[test]
fn seeding_a_directory_marked_for_another_generation_fails() {
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let first = generation(&[("@e2e/one", "1.0.0")], generated.path());
    let second = generation(&[("@e2e/two", "1.0.0")], generated.path());

    // A caller that points `PNPM_REGISTRY_STORAGE` at a directory another
    // generation already claimed a subdirectory of would otherwise have
    // the first generation's files win.
    let foreign = root.path().join(second.fingerprint());
    fs::create_dir_all(&foreign).expect("create the foreign generation directory");
    fs::write(foreign.join(".fixture-generation"), first.fingerprint())
        .expect("mark the directory for the wrong generation");

    let error = seed_generation(&second, root.path())
        .expect_err("a directory marked for another generation must not be seeded over");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(!foreign.join("@e2e/two").exists(), "no fixture files may be written into it");
}

#[test]
fn an_unmarked_directory_is_finished_rather_than_served() {
    // A seed that died partway leaves the files it reached and no marker.
    // Serving that would be a partial generation, so the next launch has to
    // complete it.
    let generated = TempDir::new().expect("create generated storage root");
    let root = TempDir::new().expect("create runtime storage root");
    let current = generation(&[("@e2e/kept", "1.0.0")], generated.path());

    let partial = root.path().join(current.fingerprint());
    let reached = partial.join("@e2e/kept");
    fs::create_dir_all(&reached).expect("create the partially seeded package dir");
    fs::copy(current.storage().join("@e2e/kept/package.json"), reached.join("package.json"))
        .expect("seed one file before dying");

    let storage = seed_generation(&current, root.path()).expect("finish the interrupted seed");
    assert_eq!(versions(storage.path(), "@e2e/kept"), ["1.0.0"]);
    assert!(
        storage
            .path()
            .join("@e2e/kept/kept-1.0.0.tgz")
            .exists(),
        "the file the interrupted seed never reached is still missing",
    );
    assert_eq!(
        fs::read_to_string(storage.path().join(".fixture-generation"))
            .expect("read the generation marker"),
        current.fingerprint(),
    );
}

/// The copy fallback is what runs where hard links are unavailable, which
/// is every destination on another device or under an ACL that forbids
/// them. It has to reach the same end state as the hard-link path, so it
/// claims the destination the same way: an existing file is the cache's
/// or a previous seed's and is left byte for byte as it is.
#[test]
fn the_copy_fallback_claims_the_destination_instead_of_overwriting_it() {
    let dir = TempDir::new().expect("create temp dir");
    let src = dir.path().join("src");
    fs::write(&src, "from the generation").expect("write the source file");

    let dest = dir.path().join("dest");
    assert!(matches!(
        copy_into_new_file(&src, &dest).expect("copy into a free path"),
        LinkOutcome::Created
    ));
    assert_eq!(fs::read_to_string(&dest).expect("read the copy"), "from the generation");

    fs::write(&dest, "a longer proxy-cache packument").expect("occupy the destination");
    assert!(matches!(
        copy_into_new_file(&src, &dest).expect("copy onto an occupied path"),
        LinkOutcome::AlreadyExists
    ));
    assert_eq!(
        fs::read_to_string(&dest).expect("read the untouched destination"),
        "a longer proxy-cache packument",
        "the copy truncated a file it should have left alone",
    );
}
