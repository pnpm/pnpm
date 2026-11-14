use super::{AddOptions, manifest, pin_to_locked_version};
use pnpm_python_resolver::{Inputs, LockedPackage, LockedWheel, Lockfile, ToolMetadata};
use std::{collections::BTreeMap, fs};

fn locked_wheel(name: &str, version: &str) -> LockedWheel {
    LockedWheel {
        name: format!("{name}-{version}-py3-none-any.whl"),
        url: format!("https://example.test/{name}-{version}-py3-none-any.whl"),
        hashes: BTreeMap::from([("sha256".to_string(), "a".repeat(64))]),
    }
}

fn locked_pkg(name: &str, version: &str, marker: Option<&str>) -> LockedPackage {
    LockedPackage {
        name: name.parse().unwrap(),
        version: version.parse().unwrap(),
        marker: marker.map(str::to_string),
        wheels: vec![locked_wheel(name, version)],
        sdist: None,
        directory: None,
        vcs: None,
    }
}

#[test]
fn manifest_add_preserves_disjoint_markers_and_replaces_canonical_matches() {
    let dir = tempfile::tempdir().unwrap();
    let pyproject = dir.path().join("pyproject.toml");
    fs::write(
        &pyproject,
        "[project]\nname = 'app'\nversion = '0.1.0'\ndependencies = [\n    \"six==1.0 ; sys_platform == 'win32'\",\n]\n",
    )
    .unwrap();

    manifest::add(&pyproject, &["six>=2.0 ; sys_platform == 'linux'".to_string()], false).unwrap();
    let parsed: toml::Value = toml::from_str(&fs::read_to_string(&pyproject).unwrap()).unwrap();
    assert_eq!(
        parsed["project"]["dependencies"].as_array().unwrap(),
        &[
            toml::Value::String("six==1.0 ; sys_platform == 'win32'".to_string()),
            toml::Value::String("six>=2.0 ; sys_platform == 'linux'".to_string()),
        ],
    );

    manifest::add(&pyproject, &[r#"six>=2.1 ; "linux" == sys_platform"#.to_string()], false)
        .unwrap();
    let parsed: toml::Value = toml::from_str(&fs::read_to_string(&pyproject).unwrap()).unwrap();
    assert_eq!(
        parsed["project"]["dependencies"].as_array().unwrap(),
        &[
            toml::Value::String("six==1.0 ; sys_platform == 'win32'".to_string()),
            toml::Value::String(r#"six>=2.1 ; "linux" == sys_platform"#.to_string()),
        ],
    );

    manifest::add(&pyproject, &["six>=1.5".to_string()], false).unwrap();
    manifest::add(&pyproject, &["six>=3.0".to_string()], false).unwrap();
    let parsed: toml::Value = toml::from_str(&fs::read_to_string(&pyproject).unwrap()).unwrap();
    assert_eq!(
        parsed["project"]["dependencies"].as_array().unwrap(),
        &[
            toml::Value::String("six==1.0 ; sys_platform == 'win32'".to_string()),
            toml::Value::String(r#"six>=2.1 ; "linux" == sys_platform"#.to_string()),
            toml::Value::String("six>=3.0".to_string()),
        ],
    );
}

#[test]
fn pin_to_locked_version_respects_direct_urls_and_environment_markers() {
    let lock = Lockfile {
        lock_version: "1.0".to_string(),
        created_by: "pnpm".to_string(),
        environments: vec![
            "python_version == '3.12' and sys_platform == 'linux'".to_string(),
            "python_version == '3.12' and sys_platform == 'win32'".to_string(),
        ],
        requires_python: None,
        packages: vec![
            locked_pkg("six", "1.0", Some("sys_platform == 'win32'")),
            locked_pkg("six", "2.0", Some("sys_platform == 'linux'")),
        ],
        tool: ToolMetadata { pnpm: Inputs::declared(&[], &[], &[], "https://pypi.org/simple/") },
    };
    let lock_index = lock.index().unwrap();
    let exact_opts =
        AddOptions { requirements: Vec::new(), development: false, exact: true, prefix: None };

    let mut url_req: pep508_rs::Requirement =
        "six @ https://example.test/six-1.16.0-py2.py3-none-any.whl".parse().unwrap();
    pin_to_locked_version(&mut url_req, &lock_index, &exact_opts, ">=").unwrap();
    assert_eq!(url_req.to_string(), "six @ https://example.test/six-1.16.0-py2.py3-none-any.whl");

    let mut unmarked_req: pep508_rs::Requirement = "six>=1.0".parse().unwrap();
    pin_to_locked_version(&mut unmarked_req, &lock_index, &exact_opts, ">=").unwrap();
    assert_eq!(unmarked_req.to_string(), "six>=1.0");

    let mut linux_req: pep508_rs::Requirement = "six ; sys_platform == 'linux'".parse().unwrap();
    let default_opts = AddOptions { exact: false, ..exact_opts };
    pin_to_locked_version(&mut linux_req, &lock_index, &default_opts, ">=").unwrap();
    assert_eq!(linux_req.to_string(), "six>=2.0 ; sys_platform == 'linux'");

    let mut darwin_req: pep508_rs::Requirement = "six ; sys_platform == 'darwin'".parse().unwrap();
    pin_to_locked_version(&mut darwin_req, &lock_index, &default_opts, ">=").unwrap();
    assert_eq!(darwin_req.to_string(), "six ; sys_platform == 'darwin'");
}
