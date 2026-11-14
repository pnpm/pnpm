use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use pep440_rs::{Version, VersionSpecifiers};
use pnpm_config::Config;
use pnpm_python_resolver::{Inputs, LockedPackage, LockedWheel, Lockfile, ToolMetadata};

use super::{
    StageContext, UpdateOptions, collect_updated_markers, finalize_requirement,
    specifiers::updated_existing_specifiers, stage_manifests_for_resolution, stage_requirement,
};
use crate::{PythonDependencyGroup, declared_dependencies, discover};

fn update(existing: &str, version: &str, force_bump: bool) -> Option<String> {
    let existing: VersionSpecifiers = existing.parse().unwrap();
    let version: Version = version.parse().unwrap();
    updated_existing_specifiers(&existing, &version, force_bump)
        .unwrap()
        .map(|specifiers| specifiers.to_string())
}

#[test]
fn compatible_release_keeps_its_precision() {
    assert_eq!(update("~=1.0", "1.5.9", false).as_deref(), Some("~=1.5"));
    assert_eq!(update("~=1.0", "2", true).as_deref(), Some("~=2.0"));
    assert_eq!(update("~=1.0.0", "1.5", false).as_deref(), Some("~=1.5.0"));
    assert_eq!(update("~=1.0", "2.0.0rc1", true).as_deref(), Some("~=2.0rc1"));
    assert_eq!(update("~=1.0", "2.0rc1", true).as_deref(), Some("~=2.0rc1"));
}

#[test]
fn compatible_release_keeps_a_selected_prerelease() {
    assert_eq!(update("~=1.0", "2.1.3rc1", true).as_deref(), Some("~=2.1rc1"));
}

#[test]
fn compatible_release_never_lowers_its_bound() {
    assert_eq!(update("~=1.2.post1", "1.2.0.1", false), None);
}

#[test]
fn compatible_release_follows_an_explicit_downgrade() {
    assert_eq!(update("~=3.0", "1.5", true).as_deref(), Some("~=1.5"));
}

#[test]
fn wildcard_equality_keeps_its_operator() {
    assert_eq!(update("==1.*", "2.3.1", true).as_deref(), Some("==2.*"));
    assert_eq!(update("==1.2.*", "2.3.1", true).as_deref(), Some("==2.3.*"));
    assert_eq!(update("==1.*", "2.1rc1", true).as_deref(), Some("==2.*, >=2.1rc1"));
    assert_eq!(update("==1.*", "1.4", true), None);
    assert_eq!(update("==1.*", "2.0", false), None);
}

#[test]
fn upper_bound_becomes_a_lower_bound_past_it() {
    assert_eq!(update("<2", "2.0", true).as_deref(), Some(">=2.0"));
}

#[test]
fn a_selected_prerelease_stays_admitted() {
    assert_eq!(update("==1.*", "1.4rc1", true).as_deref(), Some("==1.*, >=1.4rc1"));
    assert_eq!(update("<2", "1.4rc1", true).as_deref(), Some(">=1.4rc1, <2"));
    assert_eq!(update("==1.*, >=1.4rc1", "1.4rc1", true), None);
    assert_eq!(update(">=1.0", "1.4rc1", true).as_deref(), Some(">=1.4rc1"));
    assert_eq!(update(">=1.5rc1, <3.0rc1", "2.0rc1", false).as_deref(), Some(">=2.0rc1, <3.0rc1"));
}

#[test]
fn a_bounded_range_moves_its_lower_bound() {
    assert_eq!(update(">=1.0rc1, <2.0", "1.5", false).as_deref(), Some(">=1.5, <2.0"));
    assert_eq!(update("==2.*, >=2.1rc1", "2.3", false).as_deref(), Some("==2.*, >=2.3"));
    assert_eq!(update("==2.*, >=2.1rc1", "2.0", true).as_deref(), Some(">=2.0"));
    assert_eq!(update(">=1.0, <2", "2.0", true).as_deref(), Some(">=2.0"));
}

fn locked_pkg(name: &str, version: &str, marker: Option<&str>) -> LockedPackage {
    LockedPackage {
        name: name.parse().unwrap(),
        version: version.parse().unwrap(),
        marker: marker.map(str::to_string),
        wheels: vec![LockedWheel {
            name: format!("{name}-{version}-py3-none-any.whl"),
            url: format!("https://example.test/{name}-{version}-py3-none-any.whl"),
            hashes: BTreeMap::from([("sha256".to_string(), "a".repeat(64))]),
        }],
        sdist: None,
        directory: None,
        vcs: None,
    }
}

fn sample_lock(environments: Vec<&str>, packages: Vec<LockedPackage>) -> Lockfile {
    Lockfile {
        lock_version: "1.0".to_string(),
        created_by: "pnpm".to_string(),
        environments: environments
            .into_iter()
            .map(str::to_string)
            .collect(),
        requires_python: None,
        packages,
        tool: ToolMetadata { pnpm: Inputs::declared(&[], &[], &[], "https://pypi.org/simple/") },
    }
}

#[test]
fn unselected_sibling_staging_preserves_platform_specific_locked_versions() {
    let lock = sample_lock(
        vec![
            "python_version == '3.12' and sys_platform == 'linux'",
            "python_version == '3.12' and sys_platform == 'win32'",
        ],
        vec![
            locked_pkg("beta", "1.0", Some("sys_platform == 'linux'")),
            locked_pkg("beta", "2.0", Some("sys_platform == 'win32'")),
            locked_pkg("gamma", "3.0", None),
        ],
    );
    let lock_index = lock.index().unwrap();
    let options = UpdateOptions {
        targets: BTreeMap::from([("alpha".parse().unwrap(), None)]),
        restrict_to_targets: true,
        groups: BTreeSet::from([PythonDependencyGroup::Prod]),
        latest: false,
        no_save: false,
    };

    let staged_beta = stage_requirement(
        PythonDependencyGroup::Prod,
        "beta>=1.0".parse().unwrap(),
        false,
        &options,
        None,
        Some(&lock_index),
    )
    .unwrap()
    .unwrap();
    let linux_env: pep508_rs::MarkerTree =
        "python_version == '3.12' and sys_platform == 'linux'".parse().unwrap();
    let win_env: pep508_rs::MarkerTree =
        "python_version == '3.12' and sys_platform == 'win32'".parse().unwrap();
    let active_on_linux: Vec<String> = staged_beta
        .iter()
        .filter(|req| !req.marker.is_disjoint(&linux_env))
        .map(|req| {
            req.version_or_url
                .as_ref()
                .unwrap()
                .to_string()
        })
        .collect();
    let active_on_win: Vec<String> = staged_beta
        .iter()
        .filter(|req| !req.marker.is_disjoint(&win_env))
        .map(|req| {
            req.version_or_url
                .as_ref()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(active_on_linux, vec!["==1.0"]);
    assert_eq!(active_on_win, vec!["==2.0"]);

    let staged_gamma = stage_requirement(
        PythonDependencyGroup::Prod,
        "gamma>=1.0".parse().unwrap(),
        false,
        &options,
        None,
        Some(&lock_index),
    )
    .unwrap()
    .unwrap();
    let darwin_env: pep508_rs::MarkerTree =
        "python_version == '3.12' and sys_platform == 'darwin'".parse().unwrap();
    let gamma_on_linux: Vec<String> = staged_gamma
        .iter()
        .filter(|req| !req.marker.is_disjoint(&linux_env))
        .map(|req| {
            req.version_or_url
                .as_ref()
                .unwrap()
                .to_string()
        })
        .collect();
    let gamma_on_darwin: Vec<String> = staged_gamma
        .iter()
        .filter(|req| !req.marker.is_disjoint(&darwin_env))
        .map(|req| {
            req.version_or_url
                .as_ref()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(gamma_on_linux, vec!["==3.0"]);
    assert_eq!(gamma_on_darwin, vec![">=1.0"]);
}

#[test]
fn sibling_pins_preserve_disjoint_markers_and_skip_stale_constraints() {
    let lock = sample_lock(
        vec![
            "python_version == '3.12' and sys_platform == 'linux'",
            "python_version == '3.12' and sys_platform == 'win32'",
        ],
        vec![
            locked_pkg("alpha", "1.0", Some("sys_platform == 'linux'")),
            locked_pkg("alpha", "1.0", Some("sys_platform == 'win32'")),
            locked_pkg("beta", "1.0", None),
        ],
    );
    let lock_index = lock.index().unwrap();
    let options = UpdateOptions {
        targets: BTreeMap::from([("alpha".parse().unwrap(), None)]),
        restrict_to_targets: true,
        groups: BTreeSet::from([PythonDependencyGroup::Prod]),
        latest: false,
        no_save: false,
    };
    let updated_in_env =
        BTreeMap::from([("alpha".parse().unwrap(), "sys_platform == 'linux'".parse().unwrap())]);

    let win_sibling = stage_requirement(
        PythonDependencyGroup::Prod,
        "alpha>=1.0 ; sys_platform == 'win32'".parse().unwrap(),
        false,
        &options,
        Some(&updated_in_env),
        Some(&lock_index),
    )
    .unwrap()
    .unwrap();
    let win_env: pep508_rs::MarkerTree =
        "python_version == '3.12' and sys_platform == 'win32'".parse().unwrap();
    let active_on_win: Vec<String> = win_sibling
        .iter()
        .filter(|req| !req.marker.is_disjoint(&win_env))
        .map(|req| {
            req.version_or_url
                .as_ref()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(active_on_win, vec!["==1.0"]);

    let stale = stage_requirement(
        PythonDependencyGroup::Prod,
        "beta>=2.0".parse().unwrap(),
        false,
        &options,
        Some(&updated_in_env),
        Some(&lock_index),
    )
    .unwrap();
    assert!(stale.is_none());
}

#[test]
fn finalize_requirement_saves_ranges_when_platforms_resolve_different_versions() {
    let lock = sample_lock(
        vec![
            "python_version == '3.12' and sys_platform == 'linux'",
            "python_version == '3.12' and sys_platform == 'win32'",
        ],
        vec![
            locked_pkg("foo", "2.0", Some("sys_platform == 'linux'")),
            locked_pkg("foo", "3.0", Some("sys_platform == 'win32'")),
        ],
    );
    let lock_index = lock.index().unwrap();

    let explicit_opts = UpdateOptions {
        targets: BTreeMap::from([("foo".parse().unwrap(), Some(">=2".to_string()))]),
        restrict_to_targets: true,
        groups: BTreeSet::from([PythonDependencyGroup::Prod]),
        latest: false,
        no_save: false,
    };
    let saved_explicit = finalize_requirement(
        PythonDependencyGroup::Prod,
        "foo<2".parse().unwrap(),
        &explicit_opts,
        &lock_index,
    )
    .unwrap()
    .unwrap();
    assert_eq!(saved_explicit.to_string(), "foo>=2");

    let latest_opts = UpdateOptions {
        targets: BTreeMap::from([("foo".parse().unwrap(), None)]),
        restrict_to_targets: true,
        groups: BTreeSet::from([PythonDependencyGroup::Prod]),
        latest: true,
        no_save: false,
    };
    let saved_latest = finalize_requirement(
        PythonDependencyGroup::Prod,
        "foo==1.0".parse().unwrap(),
        &latest_opts,
        &lock_index,
    )
    .unwrap()
    .unwrap();
    assert_eq!(saved_latest.to_string(), "foo>=2.0");

    let pre_lock = sample_lock(
        vec![
            "python_version == '3.12' and sys_platform == 'linux'",
            "python_version == '3.12' and sys_platform == 'win32'",
        ],
        vec![
            locked_pkg("foo", "2.0", Some("sys_platform == 'linux'")),
            locked_pkg("foo", "3.0rc1", Some("sys_platform == 'win32'")),
        ],
    );
    let pre_index = pre_lock.index().unwrap();
    let saved_pre = finalize_requirement(
        PythonDependencyGroup::Prod,
        "foo==1.0".parse().unwrap(),
        &latest_opts,
        &pre_index,
    )
    .unwrap()
    .unwrap();
    let Some(pep508_rs::VersionOrUrl::VersionSpecifier(specs)) = saved_pre.version_or_url else {
        panic!("expected version specifiers");
    };
    assert!(specs.contains(&"2.0".parse().unwrap()));
    assert!(specs.contains(&"3.0rc1".parse().unwrap()));
    assert!(!specs.contains(&"1.9".parse().unwrap()));
    assert!(specs.iter().any(pep440_rs::VersionSpecifier::any_prerelease));
}

fn write_pyproject(dir: &Path, contents: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("pyproject.toml"), contents).unwrap();
}

#[tokio::test]
async fn updated_names_are_scoped_to_each_lock_root() {
    let temp = tempfile::tempdir().unwrap();
    let ws1 = temp.path().join("ws1");
    let ws2 = temp.path().join("ws2");
    let member_a = ws1.join("a");
    let member_b = ws2.join("b");
    let member_c = ws2.join("c");

    let shared_root_toml = |members: &str| {
        format!(
            "[tool.uv.workspace]\nmembers = [{members}]\n[tool.pnpm.python]\nshared-environment = true\n",
        )
    };
    write_pyproject(&ws1, &shared_root_toml(r#""a""#));
    write_pyproject(
        &member_a,
        "[project]\nname = 'pkg-a'\nversion = '0.1.0'\ndependencies = ['alpha>=1.0']\n",
    );
    write_pyproject(&ws2, &shared_root_toml(r#""b", "c""#));
    write_pyproject(
        &member_b,
        "[project]\nname = 'pkg-b'\nversion = '0.1.0'\ndependencies = ['beta>=1.0']\n",
    );
    write_pyproject(
        &member_c,
        "[project]\nname = 'pkg-c'\nversion = '0.1.0'\ndependencies = ['alpha>=1.0']\n",
    );

    let manifests =
        [ws1.as_path(), member_a.as_path(), ws2.as_path(), member_b.as_path(), member_c.as_path()]
            .into_iter()
            .map(|dir| dir.join("pyproject.toml"))
            .collect::<Vec<_>>();
    let config = Config { workspace_dir: Some(temp.path().to_path_buf()), ..Config::default() };
    let discovery = discover(&config, temp.path(), &manifests).await.unwrap();

    let selected = BTreeSet::from([member_a.clone(), member_b.clone()]);
    let options = UpdateOptions {
        targets: BTreeMap::from([
            ("alpha".parse().unwrap(), None),
            ("beta".parse().unwrap(), None),
        ]),
        restrict_to_targets: true,
        groups: BTreeSet::from([PythonDependencyGroup::Prod]),
        latest: false,
        no_save: false,
    };
    let updated_markers = collect_updated_markers(&discovery, &selected, &options).unwrap();
    let stage = StageContext {
        discovery: &discovery,
        selected: &selected,
        updated_markers,
        options: &options,
        locks: BTreeMap::from([(
            ws2.clone(),
            sample_lock(Vec::new(), vec![locked_pkg("alpha", "1.0", None)]),
        )]),
    };
    let members = BTreeSet::from([member_a, member_b, member_c.clone()]);
    stage_manifests_for_resolution(&members, &stage).unwrap();

    let staged_c: toml::Value =
        toml::from_str(&fs::read_to_string(member_c.join("pyproject.toml")).unwrap()).unwrap();
    assert_eq!(
        staged_c["project"]["dependencies"].as_array().unwrap(),
        &[toml::Value::String("alpha==1.0".to_string())],
    );
}

#[test]
fn declared_dependencies_stops_workspace_inheritance_at_workspace_boundary() {
    let temp = tempfile::tempdir().unwrap();
    write_pyproject(
        temp.path(),
        "[tool.uv.workspace]\nmembers = ['nested/*']\n[tool.uv.sources]\nfoo = { git = 'https://example.test/foo.git' }\n",
    );
    let nested = temp.path().join("nested");
    let app = nested.join("app");
    write_pyproject(
        &app,
        "[project]\nname = 'app'\nversion = '0.1.0'\ndependencies = ['foo>=1.0']\n",
    );

    let bounded = declared_dependencies(&app.join("pyproject.toml"), Some(&nested)).unwrap();
    assert_eq!(bounded.len(), 1);
    assert_eq!(bounded[0].name.to_string(), "foo");

    let unbounded_to_outer =
        declared_dependencies(&app.join("pyproject.toml"), Some(temp.path())).unwrap();
    assert!(unbounded_to_outer.is_empty());
}
