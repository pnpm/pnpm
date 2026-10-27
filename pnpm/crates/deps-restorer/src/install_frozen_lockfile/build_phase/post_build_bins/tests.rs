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
        let output = run_bin(&modules.join(".bin/tool"));
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
    let output = run_bin(&modules.join(".bin/tool"));
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

#[test]
fn private_hoist_refresh_updates_symlinked_virtual_store_package_bins() {
    let temp = tempfile::tempdir().unwrap();
    let virtual_store = temp.path().join("node_modules/.pnpm");
    let pkg_dir = virtual_store.join("tool@1.0.0/node_modules/tool");
    let hoisted_modules = virtual_store.join("node_modules");
    let symlink_location = hoisted_modules.join("tool");
    fs::create_dir_all(&pkg_dir).unwrap();
    fs::create_dir_all(&hoisted_modules).unwrap();
    let manifest = serde_json::json!({"name": "tool", "version": "1.0.0", "bin": {"tool": "tool"}});
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).unwrap();
    fs::write(pkg_dir.join("tool"), "#!/usr/bin/env node\nconsole.log('placeholder')\n").unwrap();
    pnpm_fs::symlink_dir(&pkg_dir, &symlink_location).unwrap();

    let source =
        pnpm_cmd_shim::PackageBinSource::new(symlink_location, std::sync::Arc::new(manifest))
            .with_resolved_location(pkg_dir.clone());
    let options = LinkBinsOptions::default();
    pnpm_cmd_shim::link_bins_of_packages::<Host>(
        std::slice::from_ref(&source),
        &hoisted_modules.join(".bin"),
        &options,
    )
    .unwrap();

    let key: PackageKey = "tool@1.0.0".parse().unwrap();
    let plans = crate::build_options::HoistedBinPlans::default();
    plans.seed(&HashMap::from([(hoisted_modules.clone(), vec![source].into())]));
    plans.seed_private_hoist(hoisted_modules.clone(), &[("tool".to_string(), key.clone())]);
    plans.initialize_private_hoist_for(&key).unwrap();

    fs::write(pkg_dir.join("tool"), "#!/bin/sh\nprintf 'private hoist refreshed\\n'\n").unwrap();
    plans.refresh_private_hoist_for(&key, &options).unwrap();
    plans
        .refresh_private_hoist(&HashSet::from([key]), &options)
        .unwrap();

    let output = run_bin(&hoisted_modules.join(".bin/tool"));
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"private hoist refreshed\n");
}

fn run_bin(path: &std::path::Path) -> std::process::Output {
    let mut last_busy = None;
    for _ in 0..200 {
        match Command::new(path).output() {
            Ok(output) => return output,
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                last_busy = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("run refreshed bin {}: {error:?}", path.display()),
        }
    }
    panic!("refreshed bin stayed busy {}: {last_busy:?}", path.display());
}
