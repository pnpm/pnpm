use crate::{
    candidates::{candidates_from_page, wheel_identity},
    lockfile::Target,
    metadata::WheelMetadata,
    packages::Packages,
    resolve::{Step, step},
};
use pep440_rs::Version;
use pep508_rs::{MarkerEnvironment, PackageName, Requirement};
use std::{collections::BTreeMap, str::FromStr};
use url::Url;

/// A `CPython` 3.12 target that takes a pure-Python wheel, preferring a
/// manylinux build of the same version.
fn target() -> Target {
    let environment = serde_json::from_value(serde_json::json!({
        "implementation_name": "cpython",
        "implementation_version": "3.12.0",
        "os_name": "posix",
        "platform_machine": "x86_64",
        "platform_release": "6.1.0",
        "platform_system": "Linux",
        "platform_version": "#1 SMP",
        "python_full_version": "3.12.0",
        "platform_python_implementation": "CPython",
        "python_version": "3.12",
        "sys_platform": "linux",
    }))
    .expect("marker environment fixture");
    Target {
        environment,
        tags: vec!["cp312-cp312-manylinux_2_17_x86_64".to_string(), "py3-none-any".to_string()],
    }
}

fn page(files: &serde_json::Value) -> String {
    serde_json::json!({ "files": files }).to_string()
}

fn wheel(filename: &str) -> serde_json::Value {
    serde_json::json!({
        "filename": filename,
        "url": filename,
        "hashes": { "sha256": "a".repeat(64) },
    })
}

fn name(distribution: &str) -> PackageName {
    PackageName::from_str(distribution).expect("distribution name fixture")
}

fn index_url() -> Url {
    Url::parse("https://example.test/simple/demo/").expect("index URL fixture")
}

#[test]
fn metadata_reads_continued_fields_and_stops_at_the_description() {
    let metadata = WheelMetadata::parse(concat!(
        "Metadata-Version: 2.1\n",
        "Name: demo\n",
        "Version: 1.0.0\n",
        "Requires-Python: >=3.9\n",
        "Provides-Extra: extras\n",
        "Requires-Dist: chained >=1 ;\n",
        "  extra == 'extras'\n",
        "\n",
        "Requires-Dist: not-a-requirement\n",
    ))
    .expect("metadata parses");

    assert_eq!(metadata.name, "demo");
    assert_eq!(metadata.version, "1.0.0");
    assert_eq!(metadata.requires_python.as_deref(), Some(">=3.9"));
    assert_eq!(metadata.provides_extra, ["extras"]);
    assert_eq!(metadata.requires_dist, ["chained >=1 ; extra == 'extras'"]);
}

#[test]
fn metadata_without_a_distribution_is_refused() {
    let error = WheelMetadata::parse("Metadata-Version: 2.1\n").expect_err("no name or version");
    assert!(error.to_string().contains("names no distribution"), "{error}");
}

#[test]
fn candidates_prefer_the_first_tag_the_target_lists() {
    let candidates = candidates_from_page(
        &page(&serde_json::json!([
            wheel("demo-1.0.0-py3-none-any.whl"),
            wheel("demo-1.0.0-cp312-cp312-manylinux_2_17_x86_64.whl"),
        ])),
        &index_url(),
        &name("demo"),
        &target(),
    )
    .expect("page parses");

    let candidate = &candidates[&Version::from_str("1.0.0").unwrap()];
    assert_eq!(candidate.wheel.name, "demo-1.0.0-cp312-cp312-manylinux_2_17_x86_64.whl");
    assert_eq!(
        candidate.wheel.url,
        "https://example.test/simple/demo/demo-1.0.0-cp312-cp312-manylinux_2_17_x86_64.whl",
    );
}

#[test]
fn candidates_leave_out_what_the_target_cannot_install() {
    let mut yanked = wheel("demo-2.0.0-py3-none-any.whl");
    yanked["yanked"] = serde_json::json!("withdrawn");
    let mut too_new = wheel("demo-3.0.0-py3-none-any.whl");
    too_new["requires-python"] = serde_json::json!(">=3.13");

    let candidates = candidates_from_page(
        &page(&serde_json::json!([
            wheel("demo-1.0.0-py3-none-any.whl"),
            yanked,
            too_new,
            wheel("demo-4.0.0-cp39-cp39-manylinux_2_17_x86_64.whl"),
            serde_json::json!({
                "filename": "demo-5.0.0.tar.gz",
                "url": "demo-5.0.0.tar.gz",
                "hashes": { "sha256": "b".repeat(64) },
            }),
        ])),
        &index_url(),
        &name("demo"),
        &target(),
    )
    .expect("page parses");

    assert_eq!(
        candidates.keys().map(ToString::to_string).collect::<Vec<_>>(),
        ["1.0.0"],
        "yanked, interpreter-incompatible, foreign-tag, and non-wheel files are left out",
    );
}

#[test]
fn candidates_carry_the_metadata_file_an_index_advertises() {
    let mut declared = wheel("demo-1.0.0-py3-none-any.whl");
    declared["core-metadata"] = serde_json::json!({ "sha256": "c".repeat(64) });
    let mut legacy = wheel("demo-2.0.0-py3-none-any.whl");
    legacy["dist-info-metadata"] = serde_json::json!(true);
    let plain = wheel("demo-3.0.0-py3-none-any.whl");

    let candidates = candidates_from_page(
        &page(&serde_json::json!([declared, legacy, plain])),
        &index_url(),
        &name("demo"),
        &target(),
    )
    .expect("page parses");

    let digests = candidates[&Version::from_str("1.0.0").unwrap()]
        .core_metadata
        .as_ref()
        .expect("declared with digests");
    assert_eq!(digests["sha256"], "c".repeat(64));
    assert_eq!(
        candidates[&Version::from_str("2.0.0").unwrap()].core_metadata,
        Some(BTreeMap::new()),
        "the legacy spelling declares the file without digests",
    );
    assert_eq!(candidates[&Version::from_str("3.0.0").unwrap()].core_metadata, None);
}

#[test]
fn wheel_identity_refuses_a_filename_that_is_not_one() {
    let error = wheel_identity("demo-1.0.0.whl", &target().tags).expect_err("too few parts");
    assert!(error.to_string().contains("invalid Python wheel filename"), "{error}");
    assert!(wheel_identity("demo-1.0.0.tar.gz", &target().tags).expect("not a wheel").is_none());
}

#[test]
fn a_resolution_asks_for_each_distribution_then_each_wheel_then_solves() {
    let target = target();
    let requirements = [Requirement::from_str("demo>=1").expect("requirement fixture")];
    let mut packages = Packages::new();

    let Step::NeedCandidates(needed) = step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("an empty resolution needs the root requirement's candidates");
    };
    assert_eq!(needed, name("demo"));

    packages.candidates.insert(
        name("demo"),
        candidates_from_page(
            &page(&serde_json::json!([wheel("demo-1.0.0-py3-none-any.whl")])),
            &index_url(),
            &name("demo"),
            &target,
        )
        .expect("page parses"),
    );
    let Step::NeedMetadata(needed, version) =
        step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("a candidate with no metadata read yet is the next thing needed");
    };
    assert_eq!((needed.as_ref(), version.to_string().as_str()), ("demo", "1.0.0"));

    packages.metadata.insert(
        (name("demo"), Version::from_str("1.0.0").unwrap()),
        WheelMetadata::parse("Name: demo\nVersion: 1.0.0\n").expect("metadata parses"),
    );
    let Step::Solved(solution) = step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("everything the project needs is known");
    };
    assert_eq!(solution[&name("demo")].to_string(), "1.0.0");
}

#[test]
fn a_project_with_no_satisfying_version_reports_why() {
    let target = target();
    let requirements = [Requirement::from_str("demo>=2").expect("requirement fixture")];
    let mut packages = Packages::new();
    packages.candidates.insert(
        name("demo"),
        candidates_from_page(
            &page(&serde_json::json!([wheel("demo-1.0.0-py3-none-any.whl")])),
            &index_url(),
            &name("demo"),
            &target,
        )
        .expect("page parses"),
    );

    let error = step(&packages, &requirements, &target.environment).expect_err("nothing satisfies");

    assert!(error.to_string().contains("Python dependency resolution failed"), "{error}");
}

/// The marker environment fixture has to keep parsing as one, or every
/// test above resolves against something the type would not accept.
#[test]
fn the_target_fixture_is_a_marker_environment() {
    let environment: &MarkerEnvironment = &target().environment;
    assert_eq!(environment.python_full_version().to_string(), "3.12.0");
}
