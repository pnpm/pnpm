use super::{WORKSPACE_MANIFEST_FILENAME, WorkspaceSettings, fs};

#[test]
fn null_task_entries_are_empty_task_definitions() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(WORKSPACE_MANIFEST_FILENAME),
        "tasks:\n  build:\n  lint: null\n  empty: {}\n  test:\n    dependsOn: ['build']\n",
    )
    .unwrap();

    let settings = WorkspaceSettings::load_at(dir.path()).unwrap().unwrap();
    let tasks = settings.tasks.as_ref().unwrap();
    for task in ["build", "lint", "empty"] {
        assert_eq!(tasks[task], crate::workspace_yaml::TaskSettings::default());
    }
    assert_eq!(tasks["test"].depends_on.as_deref(), Some(["build".to_string()].as_slice()));
}

#[test]
fn rejects_non_object_task_entries() {
    for value in ["true", "42", "'build'", "[]", "[build]"] {
        let yaml = format!("tasks:\n  build: {value}\n");
        let result = serde_saphyr::from_str::<WorkspaceSettings>(&yaml);
        dbg!(value, &result);
        assert!(result.is_err());
    }
}
