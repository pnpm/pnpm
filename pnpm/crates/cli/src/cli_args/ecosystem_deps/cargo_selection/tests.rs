use super::{
    SelectedCargoDep, SelectedCargoTarget, is_held_as_downgrade, is_held_by_patch,
    target_requirement,
};
use crate::cli_args::ecosystem_deps::{
    cargo_manifest_ops::DeclaredCargoDep, cargo_workspace_scope::tests::scope_with,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::SilentReporter;
use semver::{Version, VersionReq};
use std::path::PathBuf;

fn tower_0_4() -> DeclaredCargoDep {
    DeclaredCargoDep {
        alias: "tower-0-4".to_string(),
        package_name: "tower".to_string(),
        raw_spec: "0.4.0".to_string(),
        version_req: VersionReq::parse("0.4.0").unwrap(),
        group: DependencyGroup::Prod,
        manifest_path: PathBuf::from("Cargo.toml"),
        table: "workspace.dependencies",
    }
}

fn selection(dep: &DeclaredCargoDep, named: bool) -> SelectedCargoDep<'_> {
    SelectedCargoDep { dep, explicit_spec: None, named }
}

#[test]
fn latest_keeps_an_unnamed_renamed_alias_on_its_line() {
    let scope = scope_with(&[("tower", "tower"), ("tower", "tower-0-4")], &[]);
    let dep = tower_0_4();
    let unnamed = selection(&dep, false);
    let named = selection(&dep, true);
    let target = |selection| SelectedCargoTarget { selection, scope: &scope, locked: None };

    assert_eq!(target_requirement(&target(&unnamed), true).unwrap(), Some(dep.version_req.clone()));
    assert_eq!(target_requirement(&target(&named), true).unwrap(), None);
    assert_eq!(target_requirement(&target(&named), false).unwrap(), Some(dep.version_req.clone()));
}

#[test]
fn an_unnamed_patched_crate_is_held() {
    let dep = tower_0_4();
    let patched = scope_with(&[], &["tower"]);
    let unpatched = scope_with(&[], &[]);
    let unnamed = selection(&dep, false);
    let named = selection(&dep, true);

    assert!(is_held_by_patch::<SilentReporter>(&SelectedCargoTarget {
        selection: &unnamed,
        scope: &patched,
        locked: None,
    }));
    assert!(!is_held_by_patch::<SilentReporter>(&SelectedCargoTarget {
        selection: &named,
        scope: &patched,
        locked: None,
    }));
    assert!(!is_held_by_patch::<SilentReporter>(&SelectedCargoTarget {
        selection: &unnamed,
        scope: &unpatched,
        locked: None,
    }));
}

#[test]
fn only_an_explicit_version_moves_a_crate_below_its_current_version() {
    let dep = DeclaredCargoDep {
        raw_spec: "2.0.0-rc.1".to_string(),
        version_req: VersionReq::parse("2.0.0-rc.1").unwrap(),
        ..tower_0_4()
    };
    let scope = scope_with(&[], &[]);
    let locked = Version::parse("2.0.0-rc.1").unwrap();
    let older = Version::parse("1.0.0").unwrap();
    let implicit = selection(&dep, true);
    let explicit = SelectedCargoDep { dep: &dep, explicit_spec: Some("1"), named: true };
    let held = |selection, locked| {
        is_held_as_downgrade::<SilentReporter>(
            &SelectedCargoTarget { selection, scope: &scope, locked },
            &older,
        )
    };

    assert!(held(&implicit, Some(&locked)));
    assert!(held(&implicit, None), "without a lockfile the requirement sets the floor");
    assert!(!held(&explicit, Some(&locked)));
}
