use super::{
    HashMap, HashSet, Host, LinkBinsOptions, PackageKey, hoisted_modules_dirs, link_bins,
    link_options,
};
use std::{fs, process::Command};

#[test]
fn refreshes_the_physical_hoisted_bin_container_for_scoped_and_unscoped_packages() {
    for name in ["tool", "@scope/tool"] {
        let temp = tempfile::tempdir().expect("temporary project");
        let modules = temp.path().join("node_modules/ancestor/node_modules");
        let root = modules.join(name);
        fs::create_dir_all(&root).expect("create hoisted package");
        fs::write(
            root.join("package.json"),
            serde_json::json!({"name": name, "version": "1.0.0", "bin": {"tool": "tool"}})
                .to_string(),
        )
        .expect("write manifest");
        fs::write(root.join("tool"), "#!/usr/bin/env node\nconsole.log('placeholder')\n")
            .expect("write placeholder");
        let options = LinkBinsOptions::default();
        link_bins::<Host>(&modules, &modules.join(".bin"), &options).expect("link placeholder");
        fs::write(root.join("tool"), "#!/bin/sh\nprintf 'built binary\\n'\n")
            .expect("replace interpreter");
        let changed: PackageKey = format!("{name}@1.0.0").parse().expect("changed key");
        let consumer: PackageKey = "consumer@1.0.0".parse().expect("consumer key");
        let unrelated: PackageKey = "unrelated@1.0.0".parse().expect("unrelated key");
        let unrelated_root = temp.path().join("node_modules/unrelated");
        let roots = HashMap::from([
            (changed.clone(), vec![root]),
            (consumer.clone(), vec![temp.path().join("node_modules/consumer")]),
            (unrelated, vec![unrelated_root.clone()]),
        ]);
        let selected =
            hoisted_modules_dirs(&roots, &[changed.clone(), consumer], &HashSet::from([changed]));
        assert!(
            selected.contains(&modules),
            "the physical ancestor need not be a direct graph consumer",
        );
        assert!(!selected.contains(&unrelated_root.join("node_modules")));
        for directory in selected {
            link_bins::<Host>(&directory, &directory.join(".bin"), &link_options(&options, true))
                .expect("refresh selected bins");
        }
        let output = Command::new(modules.join(".bin/tool")).output().expect("run refreshed bin");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, b"built binary\n");
    }
}

#[test]
fn final_hoisted_refresh_reuses_discovery_and_completed_provider_reads() {
    let temp = tempfile::tempdir().unwrap();
    let modules = temp.path().join("node_modules");
    let root = modules.join("tool");
    let stable = modules.join("stable");
    for (directory, name) in [(&root, "tool"), (&stable, "stable")] {
        fs::create_dir_all(directory).unwrap();
        fs::write(
            directory.join("package.json"),
            serde_json::json!({"name": name, "bin": "tool"}).to_string(),
        )
        .unwrap();
        fs::write(directory.join("tool"), "#!/usr/bin/env node\n").unwrap();
    }
    let options = LinkBinsOptions { force: true, ..LinkBinsOptions::default() };
    link_bins::<Host>(&modules, &modules.join(".bin"), &options).unwrap();
    let plans = crate::build_options::HoistedBinPlans::default();
    plans
        .directory(&modules)
        .lock()
        .unwrap()
        .plan = Some(pnpm_cmd_shim::DirectoryBinPlan::discover::<Host>(&modules).unwrap());
    fs::write(stable.join("package.json"), "unchanged package must not be reread").unwrap();
    fs::write(root.join("tool"), "#!/bin/sh\nprintf 'refreshed\\n'\n").unwrap();
    let key: PackageKey = "tool@1.0.0".parse().unwrap();
    let roots = HashMap::from([(key.clone(), vec![root.clone()])]);
    let mutated = HashSet::from([key]);
    assert!(super::refresh_cached_directory(&plans, &modules, &roots, &mutated, &options).unwrap());
    let output = Command::new(modules.join(".bin/tool")).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"refreshed\n");
    fs::write(root.join("package.json"), "completed provider must not be reread").unwrap();
    assert!(super::refresh_cached_directory(&plans, &modules, &roots, &mutated, &options).unwrap());
    assert!(
        !super::refresh_cached_directory(
            &plans,
            &modules.join("uncovered"),
            &roots,
            &mutated,
            &options
        )
        .unwrap(),
    );
}

#[test]
fn final_importer_sources_clear_pending_without_changing_pre_consumer_candidates() {
    let modules = std::path::Path::new("project/node_modules");
    let location = modules.join("tool");
    let source = pnpm_cmd_shim::PackageBinSource::new(
        location.clone(),
        std::sync::Arc::new(serde_json::json!({ "name": "tool", "bin": "cli" })),
    )
    .with_build_pending(true);
    let plans = crate::build_options::HoistedBinPlans::default();
    plans.seed(&HashMap::from([(modules.to_owned(), vec![source].into())]));
    plans
        .directory(modules)
        .lock()
        .unwrap()
        .initialize(modules)
        .unwrap();
    let sources = super::importer_sources(&plans, modules, &["tool".into()], &[]).unwrap();
    assert!(!sources[&location].source().build_pending);
    assert!(
        plans
            .directory(modules)
            .lock()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .package_source(&location)
            .unwrap()
            .build_pending,
    );
}
