use super::private_only_names;
use pnpm_versioning::{WorkspaceProject, to_project_dir};
use std::{collections::HashSet, path::Path};

fn project(workspace_dir: &Path, dir: &str, name: &str) -> WorkspaceProject {
    WorkspaceProject {
        root_dir: workspace_dir.join(dir),
        name: Some(name.to_string()),
        version: Some("1.0.0".to_string()),
        prod_dependencies: Vec::new(),
    }
}

#[test]
fn a_name_only_private_projects_carry_is_private() {
    let workspace_dir = Path::new("/workspace");
    let projects = [
        project(workspace_dir, "packages/app", "app"),
        project(workspace_dir, "packages/lib", "lib"),
    ];
    let private_dirs = HashSet::from([to_project_dir(workspace_dir, &projects[0].root_dir)]);

    let names = private_only_names(&projects, workspace_dir, &private_dirs);

    assert_eq!(names, HashSet::from(["app".to_string()]));
}

/// Parked changelogs are keyed by manifest name, so skipping a name a public
/// project shares would leave that project's release unconfirmed forever.
#[test]
fn a_name_a_public_project_shares_is_not_private() {
    let workspace_dir = Path::new("/workspace");
    let projects =
        [project(workspace_dir, "apps/app", "app"), project(workspace_dir, "packages/app", "app")];
    let private_dirs = HashSet::from([to_project_dir(workspace_dir, &projects[0].root_dir)]);

    let names = private_only_names(&projects, workspace_dir, &private_dirs);

    assert!(names.is_empty(), "unexpected: {names:?}");
}
