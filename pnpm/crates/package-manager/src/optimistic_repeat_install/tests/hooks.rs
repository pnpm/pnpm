use super::{
    super::{Decision, current_pnpmfiles, settings::current_settings},
    check, isolated_included, setup_fresh_install, write_state_with_pnpmfiles,
};
use pnpm_testing_utils::fs::backdate_existing_files;
use pnpm_workspace_state::ProjectEntry;
use std::{collections::BTreeMap, fs};

#[test]
fn returns_skipped_when_a_pnpmfile_is_modified() {
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Isolated, "root", "1.0.0", "");
    let pnpmfile = dir.path().join(".pnpmfile.cjs");
    fs::write(&pnpmfile, "module.exports = {}\n").expect("write pnpmfile");
    let mut projects = BTreeMap::new();
    projects.insert(
        dir.path().to_string_lossy().into_owned(),
        ProjectEntry { name: Some("root".into()), version: Some("1.0.0".into()) },
    );
    write_state_with_pnpmfiles(
        dir.path(),
        backdate_existing_files(dir.path()),
        current_settings(config, pnpm_config::NodeLinker::Isolated, isolated_included(), None),
        projects,
        current_pnpmfiles(dir.path(), config),
    );
    fs::write(&pnpmfile, "module.exports = { hooks: {} }\n").expect("modify pnpmfile");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );

    assert!(matches!(
        decision,
        Decision::Skipped { reason } if reason.contains("pnpmfile changed")
    ));
}
