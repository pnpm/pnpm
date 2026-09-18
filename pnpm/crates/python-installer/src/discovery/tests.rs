use super::discover;
use pnpm_config::Config;
use std::fs;

#[tokio::test]
async fn reads_only_declared_workspace_members() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\nexclude = ['packages/excluded']\n",
    )
    .unwrap();
    let member = root.path().join("packages/app");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n").unwrap();
    let example = root.path().join("examples/broken");
    fs::create_dir_all(&example).unwrap();
    fs::write(example.join("pyproject.toml"), "not toml").unwrap();
    let vendored = root.path().join("vendor/broken");
    fs::create_dir_all(&vendored).unwrap();
    fs::write(vendored.join("pyproject.toml"), "not toml").unwrap();
    let excluded = root.path().join("packages/excluded");
    fs::create_dir_all(&excluded).unwrap();
    fs::write(excluded.join("pyproject.toml"), "not toml").unwrap();
    let fixture = root.path().join("tests/fixture");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("requirements.txt"), "{% template %}").unwrap();
    let manifests = vec![
        root.path().join("pyproject.toml"),
        member.join("pyproject.toml"),
        example.join("pyproject.toml"),
        vendored.join("pyproject.toml"),
        excluded.join("pyproject.toml"),
        fixture.join("requirements.txt"),
    ];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [member]);
}

#[tokio::test]
async fn skips_conventional_sample_and_fixture_directories_by_default() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("packages/app");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n")
        .unwrap();
    let example = root.path().join("examples/broken");
    fs::create_dir_all(&example).unwrap();
    fs::write(example.join("pyproject.toml"), "not toml").unwrap();
    let fixture = root.path().join("tests/fixture");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(fixture.join("requirements.txt"), "{% template %}").unwrap();
    let sample = root.path().join("sample/broken");
    fs::create_dir_all(&sample).unwrap();
    fs::write(sample.join("pyproject.toml"), "not toml").unwrap();
    let fixture_with_invalid_utf8 = root.path().join("fixture/invalid-utf8");
    fs::create_dir_all(&fixture_with_invalid_utf8).unwrap();
    fs::write(fixture_with_invalid_utf8.join("pyproject.toml"), [0xff]).unwrap();
    let template = root.path().join("template/project");
    fs::create_dir_all(&template).unwrap();
    fs::write(template.join("pyproject.toml"), "[project]\nname = 'template'\nversion = '1.0'\n")
        .unwrap();
    let missing = root.path().join("docs/missing/pyproject.toml");
    let manifests = vec![
        project.join("pyproject.toml"),
        example.join("pyproject.toml"),
        fixture.join("requirements.txt"),
        sample.join("pyproject.toml"),
        fixture_with_invalid_utf8.join("pyproject.toml"),
        template.join("pyproject.toml"),
        missing,
    ];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [project]);
}

#[tokio::test]
async fn a_workspace_declaration_can_include_a_conventional_directory() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['examples/*']\n",
    )
    .unwrap();
    let example = root.path().join("examples/app");
    fs::create_dir_all(&example).unwrap();
    fs::write(example.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n")
        .unwrap();
    let manifests = vec![root.path().join("pyproject.toml"), example.join("pyproject.toml")];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [example]);
}

#[tokio::test]
async fn nearest_workspace_declaration_controls_discovery() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\n",
    )
    .unwrap();
    let nested = root.path().join("packages/group");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("pyproject.toml"), "[tool.uv.workspace]\nmembers = ['apps/*']\n")
        .unwrap();
    let member = nested.join("apps/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n").unwrap();
    let nonmember = nested.join("other/broken");
    fs::create_dir_all(&nonmember).unwrap();
    fs::write(nonmember.join("pyproject.toml"), "not toml").unwrap();
    let manifests = vec![
        root.path().join("pyproject.toml"),
        nested.join("pyproject.toml"),
        member.join("pyproject.toml"),
        nonmember.join("pyproject.toml"),
    ];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [member]);
}

#[tokio::test]
async fn a_nested_workspace_declaration_can_include_ignored_descendants() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("examples/group");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("pyproject.toml"), "[tool.uv.workspace]\nmembers = ['apps/*']\n")
        .unwrap();
    let member = nested.join("apps/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n").unwrap();
    let malformed = root.path().join("examples/broken");
    fs::create_dir_all(&malformed).unwrap();
    fs::write(malformed.join("pyproject.toml"), "not toml").unwrap();
    let manifests = vec![
        nested.join("pyproject.toml"),
        member.join("pyproject.toml"),
        malformed.join("pyproject.toml"),
    ];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [member]);
}

#[tokio::test]
async fn an_excluded_nested_workspace_declaration_controls_its_descendants() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\nexclude = ['packages/group']\n",
    )
    .unwrap();
    let nested = root.path().join("packages/group");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("pyproject.toml"), "[tool.uv.workspace]\nmembers = ['apps/*']\n")
        .unwrap();
    let member = nested.join("apps/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "[project]\nname = 'app'\nversion = '1.0'\n").unwrap();
    let manifests = vec![
        root.path().join("pyproject.toml"),
        nested.join("pyproject.toml"),
        member.join("pyproject.toml"),
    ];

    let discovery = discover(&Config::default(), root.path(), &manifests).await.unwrap();

    assert_eq!(discovery.project_roots().collect::<Vec<_>>(), [member]);
}

#[tokio::test]
async fn reports_an_invalid_manifest_in_a_declared_member() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\n",
    )
    .unwrap();
    let member = root.path().join("packages/app");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), "not toml").unwrap();
    let manifests = vec![root.path().join("pyproject.toml"), member.join("pyproject.toml")];

    let Err(error) = discover(&Config::default(), root.path(), &manifests).await else {
        panic!("invalid member manifest must fail discovery");
    };

    eprintln!("{error:?}");
    assert!(error.to_string().contains("TOML parse error"), "{error:?}");
}

#[tokio::test]
async fn reports_invalid_utf8_in_a_declared_member() {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("pyproject.toml"),
        "[tool.uv.workspace]\nmembers = ['packages/*']\n",
    )
    .unwrap();
    let member = root.path().join("packages/app");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("pyproject.toml"), [0xff]).unwrap();
    let manifests = vec![root.path().join("pyproject.toml"), member.join("pyproject.toml")];

    let Err(error) = discover(&Config::default(), root.path(), &manifests).await else {
        panic!("invalid member manifest must fail discovery");
    };

    assert!(error.to_string().contains("decode "), "{error:?}");
}
