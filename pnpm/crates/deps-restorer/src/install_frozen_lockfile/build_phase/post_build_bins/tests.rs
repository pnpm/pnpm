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
