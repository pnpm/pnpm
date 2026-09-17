use crate::{
    candidates::{candidates_from_page, source_version, wheel_identity},
    lockfile::{Inputs, LockedSdist, Lockfile, Metadata, Solved, Target},
    metadata::WheelMetadata,
    packages::{Excluded, Packages},
    resolve::{Step, step},
};
use pep440_rs::Version;
use pep508_rs::{MarkerEnvironment, PackageName, Requirement};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};
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
    .expect("page parses")
    .candidates;

    let candidate = &candidates[&Version::from_str("1.0.0").unwrap()];
    assert_eq!(
        candidate.wheel().expect("an index file is a wheel").name,
        "demo-1.0.0-cp312-cp312-manylinux_2_17_x86_64.whl",
    );
    assert_eq!(
        candidate.wheel().expect("an index file is a wheel").url,
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
        candidates.candidates
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["1.0.0", "5.0.0"],
        "yanked, interpreter-incompatible and foreign-tag files are left out, \
         and a release with only a source distribution is offered it",
    );
    assert_eq!(
        candidates.excluded,
        Excluded {
            published: true,
            other_interpreters: BTreeSet::from([Version::from_str("3.0.0").unwrap()]),
            other_targets: BTreeSet::from([Version::from_str("4.0.0").unwrap()]),
        },
    );
}

/// Files a release published that pnpm cannot use, beside the one the
/// project wants: the whole page has to survive them, or one bad release
/// puts the distribution out of reach at every version.
#[test]
fn candidates_leave_out_a_file_they_cannot_read() {
    let mut unhashed = wheel("demo-2.0.0-py3-none-any.whl");
    unhashed["hashes"] = serde_json::json!({ "md5": "d".repeat(32) });
    let mut elsewhere = wheel("demo-3.0.0-py3-none-any.whl");
    elsewhere["url"] = serde_json::json!("ftp://files.test/demo-3.0.0-py3-none-any.whl");

    let candidates = candidates_from_page(
        &page(&serde_json::json!([
            wheel("demo-1.0.0-py3-none-any.whl"),
            unhashed,
            elsewhere,
            wheel("demo-4.0.0-py3-none.whl"),
            wheel("other-5.0.0-py3-none-any.whl"),
        ])),
        &index_url(),
        &name("demo"),
        &target(),
    )
    .expect("page parses")
    .candidates;

    assert_eq!(
        candidates
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["1.0.0"],
        "files with no SHA-256, a non-HTTP URL, an unreadable filename, or another \
         distribution's wheel are left out",
    );
}

/// The scenario of pnpm/pnpm#14910: releases whose `Requires-Python` is
/// not a version specifier, in the index page and in the wheel's own
/// metadata. Every reader of the field has to agree that it says nothing,
/// or a project that depends on such a release cannot be resolved,
/// installed, or locked.
#[test]
fn a_requires_python_that_does_not_parse_is_read_as_none_at_all() {
    let target = target();
    let mut malformed = wheel("demo-1.0.0-py3-none-any.whl");
    malformed["requires-python"] = serde_json::json!(">=3.6,");

    let candidates = candidates_from_page(
        &page(&serde_json::json!([malformed])),
        &index_url(),
        &name("demo"),
        &target,
    )
    .expect("page parses")
    .candidates;
    assert_eq!(
        candidates
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["1.0.0"],
    );

    let (packages, requirements, _) = solved_project("demo", "Requires-Python: >=3.6,\n");
    let Step::Solved(solution) = step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("the wheel offers the only version the project asks for");
    };
    assert_eq!(solution[&name("demo")].to_string(), "1.0.0");

    let lockfile = lockfile_for("demo", "Requires-Python: >=3.6,\n");
    assert_eq!(lockfile.environments, ["python_version == '3.12'"]);
}

#[test]
fn a_wheel_the_running_interpreter_is_outside_of_is_unavailable() {
    let target = target();
    let (packages, requirements, _) = solved_project("demo", "Requires-Python: >=3.13\n");

    let error = step(&packages, &requirements, &target.environment)
        .expect_err("the only version excludes this interpreter");

    assert!(error.to_string().contains("incompatible Python interpreter"), "{error}");
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
    .expect("page parses")
    .candidates;

    let digests = candidates[&Version::from_str("1.0.0").unwrap()]
        .core_metadata()
        .expect("declared with digests");
    assert_eq!(digests["sha256"], "c".repeat(64));
    assert_eq!(
        candidates[&Version::from_str("2.0.0").unwrap()].core_metadata().cloned(),
        Some(BTreeMap::new()),
        "the legacy spelling declares the file without digests",
    );
    assert_eq!(candidates[&Version::from_str("3.0.0").unwrap()].core_metadata().cloned(), None);
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
        .expect("page parses")
        .candidates,
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
        .expect("page parses")
        .candidates,
    );

    let error = step(&packages, &requirements, &target.environment).expect_err("nothing satisfies");

    assert!(error.to_string().contains("Python dependency resolution failed"), "{error}");
}

fn project_whose_newest_release_declares(requires_dist: &str) -> (Packages, Vec<Requirement>) {
    let target = target();
    let mut packages = Packages::new();
    packages.candidates.insert(
        name("demo"),
        candidates_from_page(
            &page(&serde_json::json!([
                wheel("demo-1.0.0-py3-none-any.whl"),
                wheel("demo-2.0.0-py3-none-any.whl"),
            ])),
            &index_url(),
            &name("demo"),
            &target,
        )
        .expect("page parses")
        .candidates,
    );
    for (version, requires_dist) in [("1.0.0", ""), ("2.0.0", requires_dist)] {
        packages.metadata.insert(
            (name("demo"), Version::from_str(version).unwrap()),
            WheelMetadata::parse(&format!("Name: demo\nVersion: {version}\n{requires_dist}"))
                .expect("metadata parses"),
        );
    }
    (packages, vec![Requirement::from_str("demo").expect("requirement fixture")])
}

#[test]
fn a_release_whose_requirements_cannot_be_read_gives_way_to_one_that_can() {
    let target = target();
    let (packages, requirements) =
        project_whose_newest_release_declares("Requires-Dist: >=1 helper\n");

    let Step::Solved(solution) = step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("the release before the unreadable one satisfies the project");
    };

    assert_eq!(solution[&name("demo")].to_string(), "1.0.0");
}

#[test]
fn a_project_whose_every_release_is_unreadable_reports_why() {
    let target = target();
    let (packages, requirements, _) = solved_project("demo", "Requires-Dist: >=1 helper\n");

    let error = step(&packages, &requirements, &target.environment).expect_err("nothing readable");

    assert!(error.to_string().contains("requirement pnpm cannot read"), "{error}");
}

/// A requirement pnpm does not implement is not one release's mistake:
/// every release declaring it names the same thing, so the project hears
/// about it instead of quietly installing an older release. Whatever
/// else the same wheel declares, in whatever order, says nothing about
/// that.
#[test]
fn an_unsupported_url_requirement_is_refused_rather_than_skipped() {
    let target = target();
    let url = "Requires-Dist: helper @ file:///helper-1.0.0-py3-none-any.whl\n";
    let unreadable = "Requires-Dist: >=1 helper\n";
    for requires_dist in
        [url.to_string(), format!("{unreadable}{url}"), format!("{url}{unreadable}")]
    {
        eprintln!("Requires-Dist:\n{requires_dist}");
        let (packages, requirements) = project_whose_newest_release_declares(&requires_dist);

        let error = step(&packages, &requirements, &target.environment).expect_err("unsupported");

        assert!(
            error.to_string().contains("unsupported scheme in direct URL Python requirement"),
            "{error}",
        );
    }
}

/// The marker environment fixture has to keep parsing as one, or every
/// test above resolves against something the type would not accept.
#[test]
fn the_target_fixture_is_a_marker_environment() {
    let environment: &MarkerEnvironment = &target().environment;
    assert_eq!(environment.python_full_version().to_string(), "3.12.0");
}

/// A solved one-package project: `demo 1.0.0`, whose `Requires-Dist` is
/// `requires_dist`, asked for by `requirement`.
fn solved_project(
    requirement: &str,
    requires_dist: &str,
) -> (Packages, Vec<Requirement>, BTreeMap<PackageName, Version>) {
    let target = target();
    let mut packages = Packages::new();
    packages.candidates.insert(
        name("demo"),
        candidates_from_page(
            &page(&serde_json::json!([wheel("demo-1.0.0-py3-none-any.whl")])),
            &index_url(),
            &name("demo"),
            &target,
        )
        .expect("page parses")
        .candidates,
    );
    packages.metadata.insert(
        (name("demo"), Version::from_str("1.0.0").unwrap()),
        WheelMetadata::parse(&format!("Name: demo\nVersion: 1.0.0\n{requires_dist}"))
            .expect("metadata parses"),
    );
    let requirements = vec![Requirement::from_str(requirement).expect("requirement fixture")];
    let solution = BTreeMap::from([(name("demo"), Version::from_str("1.0.0").unwrap())]);
    (packages, requirements, solution)
}

fn lockfile_for(requirement: &str, requires_dist: &str) -> Lockfile {
    let target = target();
    let (packages, requirements, solution) = solved_project(requirement, requires_dist);
    let inputs = Inputs::new(&requirements, &target, index_url().as_str());
    Lockfile::new(&packages, &target, &requirements, solution, inputs, Some(">=3.10".to_string()))
        .expect("lockfile builds")
}

#[test]
fn a_lockfile_names_the_interpreter_version_and_the_markers_its_graph_reads() {
    let cases = [
        ("demo", "", "python_version == '3.12'"),
        (
            "demo",
            "Requires-Dist: helper; sys_platform == 'win32'\n",
            "python_version == '3.12' and sys_platform == 'linux'",
        ),
        (
            "demo; platform_release >= '5'",
            "Requires-Dist: helper; extra == 'fast' and os_name == 'nt'\n",
            "os_name == 'posix' and platform_release == '6.1.0' and python_version == '3.12'",
        ),
    ];
    for (requirement, requires_dist, expected) in cases {
        eprintln!("requirement {requirement:?}, requires-dist {requires_dist:?}");
        let lockfile = lockfile_for(requirement, requires_dist);
        assert_eq!(lockfile.environments, [expected]);
    }
}

#[test]
fn a_lockfile_applies_wherever_its_wheels_install() {
    let lockfile = lockfile_for("demo", "");
    let requirements = [Requirement::from_str("demo").unwrap()];
    let inputs = Inputs::new(&requirements, &target(), index_url().as_str());

    let mut other_kernel = target();
    other_kernel.environment = serde_json::from_value(serde_json::json!({
        "implementation_name": "cpython",
        "implementation_version": "3.12.4",
        "os_name": "posix",
        "platform_machine": "arm64",
        "platform_release": "24.5.0",
        "platform_system": "Darwin",
        "platform_version": "Darwin Kernel Version 24.5.0",
        "python_full_version": "3.12.4",
        "platform_python_implementation": "CPython",
        "python_version": "3.12",
        "sys_platform": "darwin",
    }))
    .expect("marker environment fixture");
    other_kernel.tags = vec!["py3-none-any".to_string()];
    lockfile
        .applies_to(&inputs, Some(">=3.10"), &other_kernel)
        .expect("another environment that installs the same wheel");

    let mut native_only = target();
    native_only.tags = vec!["cp312-cp312-manylinux_2_17_x86_64".to_string()];
    let error = lockfile
        .applies_to(&inputs, Some(">=3.10"), &native_only)
        .expect_err("no tag");
    assert!(error.to_string().contains("pins no wheel of demo==1.0.0"), "{error}");

    let other_requirements = [Requirement::from_str("demo>=1").unwrap()];
    let error = lockfile
        .applies_to(
            &Inputs::new(&other_requirements, &target(), index_url().as_str()),
            Some(">=3.10"),
            &target(),
        )
        .expect_err("other requirements");
    assert!(error.to_string().contains("requirements changed"), "{error}");

    let other_index = Inputs::new(&requirements, &target(), "https://other.test/simple/");
    let error = lockfile
        .applies_to(&other_index, Some(">=3.10"), &target())
        .expect_err("index");
    assert!(error.to_string().contains("index changed"), "{error}");

    let error = lockfile
        .applies_to(&inputs, None, &target())
        .expect_err("requires-python");
    assert!(error.to_string().contains("requires-python changed"), "{error}");
}

#[test]
fn a_lockfile_pinning_another_distribution_under_a_package_is_refused() {
    let mut lockfile = lockfile_for("demo", "");
    let requirements = [Requirement::from_str("demo").unwrap()];
    let inputs = Inputs::new(&requirements, &target(), index_url().as_str());
    lockfile.packages[0].wheels[0].name = "other-1.0.0-py3-none-any.whl".to_string();

    let error = lockfile
        .applies_to(&inputs, Some(">=3.10"), &target())
        .expect_err("wrong wheel");

    assert!(error.to_string().contains("wheel identity mismatch"), "{error}");
}

#[test]
fn a_lockfile_pins_the_full_interpreter_version_when_a_package_tells_patch_releases_apart() {
    let cases = [
        (">=3.10", false),
        (">=3.8.1", false),
        (">=3.12.1", true),
        ("<3.13", false),
        ("<3.13.1", false),
        ("<3.12.9", true),
        ("!=3.11.2", false),
        ("~=3.12.2", true),
        ("~=3.12", false),
        ("==3.12.*", false),
        ("!=3.11.*", false),
        (">3.11", false),
        (">3.12", true),
        ("<=3.12", true),
        ("==3.12", true),
        ("!=3.12", true),
        (">=3.10,<4", false),
        (">=3.12.0", false),
        ("<3.13.0", false),
        ("~=3.12.0", false),
        ("~=3.12.0.0", true),
        ("==3.12.0", true),
        ("==3.12.0.*", true),
        ("<3.12.0.post1", true),
        (">=3.12.0rc1", false),
        ("<3.13.0.post1", false),
    ];
    for (requires_python, pins_full_version) in cases {
        eprintln!("Requires-Python: {requires_python}");
        let lockfile = lockfile_for("demo", &format!("Requires-Python: {requires_python}\n"));
        assert_eq!(
            lockfile.environments[0].contains("python_full_version == '3.12.0'"),
            pins_full_version,
            "{}",
            lockfile.environments[0],
        );
        assert!(lockfile.environments[0].contains("python_version == '3.12'"));
    }
}

/// A `CPython` 3.12 environment a project declares, which reports no
/// kernel and takes wheels built for `tag`.
fn declared_target(sys_platform: &str, machine: &str, tag: &str) -> Target {
    let environment = serde_json::from_value(serde_json::json!({
        "implementation_name": "cpython",
        "implementation_version": "3.12.0",
        "os_name": if sys_platform == "win32" { "nt" } else { "posix" },
        "platform_machine": machine,
        "platform_release": "",
        "platform_system": if sys_platform == "win32" { "Windows" } else { "Linux" },
        "platform_version": "",
        "python_full_version": "3.12.0",
        "platform_python_implementation": "CPython",
        "python_version": "3.12",
        "sys_platform": sys_platform,
    }))
    .expect("marker environment fixture");
    Target { environment, tags: vec![tag.to_string(), "py3-none-any".to_string()] }
}

/// The marker variables a declared environment pins.
fn declared_keys() -> Vec<String> {
    ["implementation_name", "platform_machine", "python_version", "sys_platform"]
        .map(ToString::to_string)
        .to_vec()
}

fn version(release: &str) -> Version {
    Version::from_str(release).expect("version fixture")
}

/// What an index offers a target: every distribution published as the
/// wheel files named.
fn offered(target: &Target, distributions: &[(&str, &[&str])]) -> Packages {
    let mut packages = Packages::new();
    for (distribution, filenames) in distributions {
        let files = filenames
            .iter()
            .map(|filename| wheel(filename))
            .collect::<Vec<_>>();
        packages.candidates.insert(
            name(distribution),
            candidates_from_page(
                &page(&serde_json::json!(files)),
                &index_url(),
                &name(distribution),
                target,
            )
            .expect("page parses")
            .candidates,
        );
    }
    packages
}

/// `demo 1.0.0`, which needs `helper` on Windows only, published as one
/// wheel per platform.
fn platform_project() -> (Metadata, Vec<Requirement>, Vec<Solved>) {
    let demo: &[&str] =
        &["demo-1.0.0-py3-none-manylinux_2_17_x86_64.whl", "demo-1.0.0-py3-none-win_amd64.whl"];
    let helper: &[&str] = &["helper-1.0.0-py3-none-any.whl"];
    let mut metadata = Metadata::new();
    metadata.insert(
        (name("demo"), version("1.0.0")),
        WheelMetadata::parse(
            "Name: demo\nVersion: 1.0.0\nRequires-Dist: helper; sys_platform == 'win32'\n",
        )
        .expect("metadata parses"),
    );
    metadata.insert(
        (name("helper"), version("1.0.0")),
        WheelMetadata::parse("Name: helper\nVersion: 1.0.0\n").expect("metadata parses"),
    );
    let linux = declared_target("linux", "x86_64", "py3-none-manylinux_2_17_x86_64");
    let windows = declared_target("win32", "AMD64", "py3-none-win_amd64");
    let solved = vec![
        Solved::new(
            linux.clone(),
            BTreeMap::from([(name("demo"), version("1.0.0"))]),
            &offered(&linux, &[("demo", demo)]),
            declared_keys(),
        )
        .expect("the Linux environment solves"),
        Solved::new(
            windows.clone(),
            BTreeMap::from([(name("demo"), version("1.0.0")), (name("helper"), version("1.0.0"))]),
            &offered(&windows, &[("demo", demo), ("helper", helper)]),
            declared_keys(),
        )
        .expect("the Windows environment solves"),
    ];
    (metadata, vec![Requirement::from_str("demo").expect("requirement fixture")], solved)
}

fn declared_inputs(requirements: &[Requirement]) -> Inputs {
    Inputs::declared(
        requirements,
        &["x86_64-manylinux_2_17".to_string(), "x86_64-pc-windows-msvc".to_string()],
        &[],
        index_url().as_str(),
    )
}

#[test]
fn a_lockfile_for_several_environments_pins_the_wheel_each_of_them_takes() {
    let (metadata, requirements, solved) = platform_project();
    let inputs = declared_inputs(&requirements);

    let lockfile =
        Lockfile::merged(&metadata, &requirements, &solved, inputs, Some(">=3.10".to_string()))
            .expect("lockfile builds");

    dbg!(&lockfile);
    assert_eq!(
        lockfile.environments,
        [
            "implementation_name == 'cpython' and platform_machine == 'x86_64' \
             and python_version == '3.12' and sys_platform == 'linux'",
            "implementation_name == 'cpython' and platform_machine == 'AMD64' \
             and python_version == '3.12' and sys_platform == 'win32'",
        ],
    );
    let demo = &lockfile.packages[0];
    assert_eq!(demo.name.to_string(), "demo");
    assert_eq!(demo.marker, None);
    assert_eq!(
        demo.wheels
            .iter()
            .map(|wheel| wheel.name.as_str())
            .collect::<Vec<_>>(),
        ["demo-1.0.0-py3-none-manylinux_2_17_x86_64.whl", "demo-1.0.0-py3-none-win_amd64.whl"],
    );
    let helper = &lockfile.packages[1];
    assert_eq!(helper.name.to_string(), "helper");
    assert!(
        helper.marker
            .as_deref()
            .expect("helper is installed on Windows alone")
            .contains("sys_platform == 'win32'"),
        "{:?}",
        helper.marker,
    );
}

#[test]
fn an_environment_takes_only_the_packages_and_wheels_the_lockfile_gives_it() {
    let (metadata, requirements, solved) = platform_project();
    let inputs = declared_inputs(&requirements);
    let lockfile =
        Lockfile::merged(&metadata, &requirements, &solved, inputs, Some(">=3.10".to_string()))
            .expect("lockfile builds");

    let mut linux = Packages::new();
    lockfile
        .seed(&mut linux, &declared_target("linux", "x86_64", "py3-none-manylinux_2_17_x86_64"))
        .expect("the Linux environment installs");
    assert_eq!(
        linux.candidates[&name("demo")][&version("1.0.0")]
            .wheel()
            .expect("an index file is a wheel")
            .name,
        "demo-1.0.0-py3-none-manylinux_2_17_x86_64.whl",
    );
    assert!(!linux.candidates.contains_key(&name("helper")), "{:?}", linux.candidates.keys());

    let mut windows = Packages::new();
    lockfile
        .seed(&mut windows, &declared_target("win32", "AMD64", "py3-none-win_amd64"))
        .expect("the Windows environment installs");
    assert_eq!(
        windows.candidates[&name("demo")][&version("1.0.0")]
            .wheel()
            .expect("an index file is a wheel")
            .name,
        "demo-1.0.0-py3-none-win_amd64.whl",
    );
    assert!(windows.candidates.contains_key(&name("helper")));
}

#[test]
fn two_environments_nothing_tells_apart_cannot_need_different_versions() {
    let glibc = declared_target("linux", "x86_64", "py3-none-manylinux_2_17_x86_64");
    let musl = declared_target("linux", "x86_64", "py3-none-musllinux_1_2_x86_64");
    let demo: &[&str] = &[
        "demo-1.0.0-py3-none-musllinux_1_2_x86_64.whl",
        "demo-2.0.0-py3-none-manylinux_2_17_x86_64.whl",
    ];
    let mut metadata = Metadata::new();
    for release in ["1.0.0", "2.0.0"] {
        metadata.insert(
            (name("demo"), version(release)),
            WheelMetadata::parse(&format!("Name: demo\nVersion: {release}\n"))
                .expect("metadata parses"),
        );
    }
    let requirements = vec![Requirement::from_str("demo").expect("requirement fixture")];
    let solved = vec![
        Solved::new(
            glibc.clone(),
            BTreeMap::from([(name("demo"), version("2.0.0"))]),
            &offered(&glibc, &[("demo", demo)]),
            declared_keys(),
        )
        .expect("the glibc environment solves"),
        Solved::new(
            musl.clone(),
            BTreeMap::from([(name("demo"), version("1.0.0"))]),
            &offered(&musl, &[("demo", demo)]),
            declared_keys(),
        )
        .expect("the musl environment solves"),
    ];

    let error = Lockfile::merged(
        &metadata,
        &requirements,
        &solved,
        declared_inputs(&requirements),
        Some(">=3.10".to_string()),
    )
    .expect_err("one environment, two versions");

    assert!(error.to_string().contains("nothing in their markers tells them apart"), "{error}");
}

#[test]
fn a_declared_environment_that_leaves_a_marker_undecided_is_refused() {
    let cases = [
        ("demo; platform_release >= '9'", "", "platform_release"),
        ("demo", "Requires-Dist: helper; python_full_version >= '3.12.4'\n", "python_full_version"),
        ("demo", "Requires-Python: <3.12.9\n", "python_full_version"),
    ];
    for (requirement, requires_dist, undecided) in cases {
        eprintln!("requirement {requirement:?}, metadata {requires_dist:?}");
        let target = declared_target("linux", "x86_64", "py3-none-manylinux_2_17_x86_64");
        let mut metadata = Metadata::new();
        metadata.insert(
            (name("demo"), version("1.0.0")),
            WheelMetadata::parse(&format!("Name: demo\nVersion: 1.0.0\n{requires_dist}"))
                .expect("metadata parses"),
        );
        let requirements = vec![Requirement::from_str(requirement).expect("requirement fixture")];
        let solved = vec![
            Solved::new(
                target.clone(),
                BTreeMap::from([(name("demo"), version("1.0.0"))]),
                &offered(&target, &[("demo", &["demo-1.0.0-py3-none-any.whl"])]),
                declared_keys(),
            )
            .expect("the environment solves"),
        ];

        let error = Lockfile::merged(
            &metadata,
            &requirements,
            &solved,
            declared_inputs(&requirements),
            Some(">=3.10".to_string()),
        )
        .expect_err("undecided marker");

        assert!(error.to_string().contains("undecided"), "{error}");
        assert!(error.to_string().contains(undecided), "{error}");
    }
}

#[test]
fn git_and_index_sources_of_one_version_remain_distinct_across_environments() {
    let (metadata, requirements, mut solved) = platform_project();
    let vcs = crate::LockedVcs {
        kind: "git".to_string(),
        url: "https://example.test/demo.git".to_string(),
        requested_revision: "main".to_string(),
        commit_id: "a".repeat(40),
        subdirectory: None,
    };
    solved[0].wheels.remove(&name("demo"));
    solved[0].vcs.insert(name("demo"), vcs.clone());
    let lock =
        Lockfile::merged(&metadata, &requirements, &solved, declared_inputs(&requirements), None)
            .unwrap();
    assert_eq!(
        lock.packages
            .iter()
            .filter(|package| package.name == name("demo"))
            .count(),
        2,
    );
    let mut linux = Packages::new();
    lock.seed(&mut linux, &solved[0].target)
        .unwrap();
    assert!(
        matches!(&linux.candidates[&name("demo")][&version("1.0.0")], crate::Candidate::Vcs(locked) if locked == &vcs),
    );
    let mut windows = Packages::new();
    lock.seed(&mut windows, &solved[1].target)
        .unwrap();
    assert!(windows.candidates[&name("demo")][&version("1.0.0")].wheel().is_some());
}

mod frozen_sources;
#[test]
fn overrides_replace_transitive_ranges_and_constraints_only_narrow_reached_packages() {
    let target = target();
    let mut packages = offered(
        &target,
        &[
            ("demo", &["demo-1.0-py3-none-any.whl"]),
            (
                "helper",
                &[
                    "helper-1.0-py3-none-any.whl",
                    "helper-2.0-py3-none-any.whl",
                    "helper-3.0-py3-none-any.whl",
                ],
            ),
        ],
    );
    packages.metadata.insert(
        (name("demo"), version("1.0")),
        WheelMetadata::parse("Name: demo\nVersion: 1.0\nRequires-Dist: helper<2\n").unwrap(),
    );
    for release in ["1.0", "2.0", "3.0"] {
        packages.metadata.insert(
            (name("helper"), version(release)),
            WheelMetadata::parse(&format!("Name: helper\nVersion: {release}\n")).unwrap(),
        );
    }
    let requirements = vec!["demo".parse::<Requirement>().unwrap()];
    packages.overrides = vec!["helper>=2; sys_platform == 'linux'".parse().unwrap()];
    packages.constraints = vec!["helper<3".parse().unwrap(), "absent==1".parse().unwrap()];
    let solved = crate::locked_solution(&packages, &requirements, &target.environment).unwrap();
    assert_eq!(
        solved,
        BTreeMap::from([(name("demo"), version("1.0")), (name("helper"), version("2.0"))]),
    );
    crate::validate_locked(&packages, &requirements, &target.environment).unwrap();
    packages.overrides = vec!["helper>=2; sys_platform == 'win32'".parse().unwrap()];
    let solved = crate::locked_solution(&packages, &requirements, &target.environment).unwrap();
    assert_eq!(solved[&name("helper")], version("1.0"));
    packages.constraints = vec!["helper>=2".parse().unwrap()];
    let error = crate::locked_solution(&packages, &requirements, &target.environment).unwrap_err();
    eprintln!("{error}");
    assert!(error.to_string().contains("does not satisfy"));
}

fn sdist(filename: &str) -> serde_json::Value {
    serde_json::json!({
        "filename": filename,
        "url": filename,
        "hashes": { "sha256": "b".repeat(64) },
    })
}

#[test]
fn a_release_is_offered_its_source_distribution_only_where_no_wheel_fits() {
    let offered = candidates_from_page(
        &page(&serde_json::json!([
            wheel("demo-1.0.0-py3-none-any.whl"),
            sdist("demo-1.0.0.tar.gz"),
            sdist("demo-2.0.0.zip"),
            wheel("demo-3.0.0-cp39-cp39-manylinux_2_17_x86_64.whl"),
            sdist("demo-3.0.0.tar.gz"),
        ])),
        &index_url(),
        &name("demo"),
        &target(),
    )
    .expect("page parses");

    dbg!(&offered.candidates);
    assert_eq!(
        offered.candidates[&Version::from_str("1.0.0").unwrap()]
            .wheel()
            .expect("a wheel wins over the source distribution of its release")
            .name,
        "demo-1.0.0-py3-none-any.whl",
    );
    for (version, filename) in [("2.0.0", "demo-2.0.0.zip"), ("3.0.0", "demo-3.0.0.tar.gz")] {
        assert_eq!(
            offered.candidates[&Version::from_str(version).unwrap()]
                .sdist()
                .expect("a release with no wheel for this target is offered its source")
                .name,
            filename,
        );
    }
    assert_eq!(offered.excluded.releases(), 0);
}

/// The distribution that stops `getsentry/sentry`, whose name holds the
/// dashes that separate the version from it.
#[test]
fn a_source_distribution_filename_is_read_against_the_distribution_it_is_published_under() {
    assert_eq!(
        source_version("python-u2flib-server-5.0.0.tar.gz", &name("python-u2flib-server"))
            .expect("filename parses")
            .expect("a source distribution of the distribution")
            .to_string(),
        "5.0.0",
    );
    assert_eq!(
        source_version("Demo_Project-1.0.tar.gz", &name("demo-project"))
            .expect("filename parses")
            .expect("the name is compared as a distribution name, not as text")
            .to_string(),
        "1.0",
    );
    for filename in ["other-1.0.tar.gz", "demo-1.0.tar.bz2", "demo.tar.gz", "demo-1.0-py3.egg"] {
        assert!(
            source_version(filename, &name("demo")).expect("filename parses").is_none(),
            "read {filename} as a source distribution of demo",
        );
    }
}

/// Item 5 of pnpm/pnpm#14945: an empty version set says nothing about
/// whether the distribution exists, publishes anything installable, or
/// simply has no version the project asked for.
#[test]
fn a_resolution_failure_says_why_a_distribution_offered_nothing() {
    let target = target();
    let unknown = {
        let mut packages = Packages::new();
        packages.candidates.insert(name("demo"), BTreeMap::new());
        packages.excluded.insert(name("demo"), Excluded::default());
        packages
    };
    let offered_by_page = |files: serde_json::Value| {
        let mut packages = Packages::new();
        let offered = candidates_from_page(&page(&files), &index_url(), &name("demo"), &target)
            .expect("page parses");
        packages.candidates.insert(name("demo"), offered.candidates);
        packages.excluded.insert(name("demo"), offered.excluded);
        packages
    };
    let elsewhere = offered_by_page(serde_json::json!([wheel(
        "demo-1.0.0-cp39-cp39-manylinux_2_17_x86_64.whl"
    )]));
    let many = offered_by_page(serde_json::json!(
        (1..=10)
            .map(|minor| {
                wheel(&format!("demo-1.{minor}.0-cp39-cp39-manylinux_2_17_x86_64.whl"))
            })
            .collect::<Vec<_>>()
    ));
    let (unselected, _, _) = solved_project("demo>=1", "");

    for (packages, requirement, expected) in [
        (unknown, "demo>=1", "No index pnpm reads publishes a distribution named demo."),
        (
            elsewhere,
            "demo>=1",
            "demo publishes 1 releases (1.0.0), none of which publishes a wheel this interpreter \
             installs or a source distribution pnpm can build.",
        ),
        (
            unselected,
            "demo>=2",
            "demo is offered at 1.0.0, and this project's requirements select none of them.",
        ),
        (
            many,
            "demo>=1",
            "demo publishes 10 releases (1.10.0, 1.9.0, 1.8.0, 1.7.0, 1.6.0, 1.5.0, 1.4.0, \
             1.3.0 and 2 older), none of which",
        ),
    ] {
        let requirements = [Requirement::from_str(requirement).expect("requirement fixture")];
        let error = step(&packages, &requirements, &target.environment)
            .expect_err("no version satisfies the project");
        let message = error.to_string();
        assert!(message.contains(expected), "{message}");
    }
}

/// Which archive of a release a target takes can differ when the files
/// declare different interpreter ranges, and one PEP 751 entry pins one
/// archive. Merging the two silently would replay an archive the other
/// environment's own resolution rejected.
#[test]
fn environments_that_take_different_archives_of_one_release_are_refused() {
    let target = target();
    let solved = |filename: &str| {
        let mut packages = Packages::new();
        let offered = candidates_from_page(
            &page(&serde_json::json!([sdist(filename)])),
            &index_url(),
            &name("demo"),
            &target,
        )
        .expect("page parses");
        packages.candidates.insert(name("demo"), offered.candidates);
        let version = Version::from_str("1.0.0").unwrap();
        packages.metadata.insert(
            (name("demo"), version.clone()),
            WheelMetadata::parse("Name: demo\nVersion: 1.0.0\n").expect("metadata parses"),
        );
        Solved::new(
            target.clone(),
            BTreeMap::from([(name("demo"), version)]),
            &packages,
            vec!["python_version".to_string()],
        )
        .expect("the environment solved")
    };
    let requirements = vec![Requirement::from_str("demo>=1").expect("requirement fixture")];
    let solved = [solved("demo-1.0.0.tar.gz"), solved("demo-1.0.0.zip")];
    let metadata = Metadata::from([(
        (name("demo"), Version::from_str("1.0.0").unwrap()),
        WheelMetadata::parse("Name: demo\nVersion: 1.0.0\n").expect("metadata parses"),
    )]);

    let error = Lockfile::merged(
        &metadata,
        &requirements,
        &solved,
        Inputs::new(&requirements, &target, index_url().as_str()),
        None,
    )
    .expect_err("one entry cannot pin both archives");

    dbg!(&error);
    assert!(error.to_string().contains("different archives of demo"), "{error}");
}

#[test]
fn a_lockfile_pins_the_source_distribution_a_release_is_built_from() {
    let target = target();
    let mut packages = Packages::new();
    let offered = candidates_from_page(
        &page(&serde_json::json!([sdist("demo-1.0.0.tar.gz")])),
        &index_url(),
        &name("demo"),
        &target,
    )
    .expect("page parses");
    packages.candidates.insert(name("demo"), offered.candidates);
    packages.metadata.insert(
        (name("demo"), Version::from_str("1.0.0").unwrap()),
        WheelMetadata::parse("Name: demo\nVersion: 1.0.0\n").expect("metadata parses"),
    );
    let requirements = vec![Requirement::from_str("demo>=1").expect("requirement fixture")];
    let Step::Solved(solution) = step(&packages, &requirements, &target.environment).unwrap()
    else {
        panic!("the source distribution is the only candidate the project needs");
    };
    let inputs = || Inputs::new(&requirements, &target, index_url().as_str());
    let locked = || {
        Lockfile::new(&packages, &target, &requirements, solution.clone(), inputs(), None)
            .expect("lockfile builds")
    };

    let lockfile = locked();
    let [package] = lockfile.packages.as_slice() else { panic!("one package was solved") };
    assert!(package.wheels.is_empty());
    assert_eq!(
        package.sdist.as_ref().expect("the release pins its source distribution").name,
        "demo-1.0.0.tar.gz",
    );

    lockfile
        .applies_to(&inputs(), None, &target)
        .expect("the lockfile applies to this target");
    let mut seeded = Packages::new();
    lockfile.seed(&mut seeded, &target).expect("the lockfile seeds its own candidates");
    assert_eq!(
        seeded.candidates[&name("demo")][&Version::from_str("1.0.0").unwrap()]
            .sdist()
            .expect("a package pinning a source distribution is offered it")
            .name,
        "demo-1.0.0.tar.gz",
    );

    // A lockfile is untrusted input, and the store fetches a `file:` URL
    // from this machine rather than from the index.
    for tamper in [
        |sdist: &mut LockedSdist| sdist.url = "file:///etc/shadow".to_string(),
        |sdist: &mut LockedSdist| sdist.name = "other-1.0.0.tar.gz".to_string(),
        |sdist: &mut LockedSdist| sdist.name = "demo-2.0.0.tar.gz".to_string(),
    ] {
        let mut tampered = locked();
        tamper(
            tampered.packages[0].sdist.as_mut().expect("the release pins its source distribution"),
        );
        let error = tampered
            .applies_to(&inputs(), None, &target)
            .expect_err("a lockfile pointing the archive elsewhere is refused");
        dbg!(&error);
        assert!(error.to_string().contains("Python"), "{error}");
    }
}
