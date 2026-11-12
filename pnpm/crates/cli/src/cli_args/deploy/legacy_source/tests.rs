use super::{ConvertCtx, HashMap, create_file_url_key, relative_file_key};

#[test]
fn relative_file_keys_decode_spaces_in_the_source_path() {
    let root = tempfile::tempdir().unwrap();
    let source_root = root.path().join("workspace (old)");
    let source = source_root.join("source with spaces");
    let deploy = source_root.join("out");
    let projects = HashMap::new();
    let ctx = ConvertCtx {
        projects_by_path: &projects,
        deploy_dir: &deploy,
        lockfile_dir: root.path(),
        deployed_project_root: &source,
        references: crate::cli_args::deploy::lockfile::ReferenceLocation::Workspace,
    };
    let name = "fixture".parse().unwrap();
    let key = create_file_url_key(&source, "", &projects, Some(&name)).unwrap();
    assert_eq!(
        relative_file_key(&key, &ctx).unwrap().to_string(),
        "fixture@file:../source with spaces",
    );
}
