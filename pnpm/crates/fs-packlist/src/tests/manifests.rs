use super::{fs, json, packlist, tempdir, touch, write};

#[test]
fn bundle_dependencies_subtree_is_included() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "index.js");
    touch(root, "node_modules/dep/package.json");
    touch(root, "node_modules/dep/lib.js");
    touch(root, "node_modules/other/package.json");
    touch(root, "node_modules/other/lib.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["dep"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"node_modules/dep/package.json".to_string()));
    assert!(out.contains(&"node_modules/dep/lib.js".to_string()));
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/other")),
        "non-bundled `other` must not ship: {out:?}",
    );
}

#[test]
fn bundle_dependencies_optional_deps_of_bundled_dep_are_included() {
    // `optionalDependencies` are part of a bundled package's runtime
    // closure, so they ship; `devDependencies` do not.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    write(
        root,
        "node_modules/top/package.json",
        r#"{"name":"top","version":"1.0.0","optionalDependencies":{"opt":"1.0.0"},"devDependencies":{"dev":"1.0.0"}}"#,
    );
    touch(root, "node_modules/top/index.js");
    write(root, "node_modules/opt/package.json", r#"{"name":"opt","version":"1.0.0"}"#);
    touch(root, "node_modules/opt/index.js");
    write(root, "node_modules/dev/package.json", r#"{"name":"dev","version":"1.0.0"}"#);
    touch(root, "node_modules/dev/index.js");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["top"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"node_modules/opt/index.js".to_string()),
        "optionalDependencies of a bundled dep must ship: {out:?}",
    );
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/dev")),
        "devDependencies of a bundled dep must not ship: {out:?}",
    );
}

#[test]
fn bundled_dependencies_legacy_spelling_works() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "node_modules/legacy-bundle/package.json");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundledDependencies": ["legacy-bundle"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(
        out.contains(&"node_modules/legacy-bundle/package.json".to_string()),
        "`bundledDependencies` is the legacy spelling and must be accepted: {out:?}",
    );
}

#[test]
fn bundle_dependency_missing_dir_is_silently_skipped() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["ghost"],
    });
    let out = packlist(root, &manifest).unwrap();
    assert_eq!(out, vec!["package.json".to_string()]);
}

#[test]
fn bundle_dependencies_self_cycle_is_caught() {
    // Defense-in-depth: a bundled dep whose own manifest depends on
    // itself (or any cycle reachable through the canonical-path chain)
    // must not loop the closure walk forever. The visited-set keyed on
    // the canonicalised resolved directory stops the re-entry.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");
    touch(root, "node_modules/self/package.json");
    touch(root, "node_modules/self/lib.js");
    // The bundled dep lists itself as a runtime dependency. The
    // closure follows `dependencies`, so without cycle detection this
    // re-resolves `self` forever.
    fs::write(
        root.join("node_modules/self/package.json"),
        r#"{"name":"self","version":"1.0.0","dependencies":{"self":"1.0.0"}}"#,
    )
    .unwrap();
    // Symlink `node_modules/self/node_modules/self` back to the
    // outer `node_modules/self` so the nested-first walk-up resolves
    // the self-dependency to a directory the canonical-path check
    // recognises as already visited. (On platforms that can't
    // symlink, the walk-up falls back to the same outer directory and
    // the visited-set still catches it.)
    fs::create_dir_all(root.join("node_modules/self/node_modules")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            root.join("node_modules/self"),
            root.join("node_modules/self/node_modules/self"),
        )
        .unwrap();
    }

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["self"],
    });
    let out = packlist(root, &manifest).unwrap();

    assert!(out.contains(&"package.json".to_string()));
    assert!(out.contains(&"node_modules/self/package.json".to_string()));
    assert!(out.contains(&"node_modules/self/lib.js".to_string()));
    // No deeper paths via the cycle — the visited-set refused the
    // re-entry.
    assert!(
        !out.iter().any(|p| p.starts_with("node_modules/self/node_modules/")),
        "cycle through node_modules/self/node_modules/self/... must be cut: {out:?}",
    );
}

#[test]
fn bundle_dependencies_closure_stops_past_max_depth() {
    // On top of the visited-set, the closure walk carries a
    // belt-and-braces `MAX_BUNDLE_DEPTH` cap: a pathological chain of
    // `dependencies` that keeps resolving fresh, never-repeating
    // canonical paths would slip past the cycle check and descend
    // forever. Build a linear chain longer than the cap (each package
    // depends on the next) and assert the packages beyond it are
    // refused while everything within it still ships.
    let dir = tempdir().unwrap();
    let root = dir.path();
    touch(root, "package.json");

    // The cap refuses any task whose depth exceeds 32. Tasks are
    // depth-0 for the root's bundle seed and gain one level per
    // `dependencies` hop, so `p33` is the first package created beyond
    // the cap. Lay the chain out hoisted-flat under the root
    // `node_modules/`: the walk-up resolves `p{n+1}` from there while
    // the depth counter still climbs one per hop.
    const FIRST_REFUSED: usize = 33;
    const LAST: usize = FIRST_REFUSED + 1;
    for n in 0..=LAST {
        // Every package except the last depends on the next one, forming the
        // linear chain.
        let manifest = if n < LAST {
            let next = format!("p{}", n + 1);
            json!({
                "name": format!("p{n}"),
                "version": "1.0.0",
                "dependencies": { (next): "1.0.0" },
            })
        } else {
            json!({
                "name": format!("p{n}"),
                "version": "1.0.0",
            })
        };
        write(root, &format!("node_modules/p{n}/package.json"), &manifest.to_string());
        touch(root, &format!("node_modules/p{n}/index.js"));
    }

    let manifest = json!({
        "name": "x",
        "version": "0.0.0",
        "bundleDependencies": ["p0"],
    });
    let out = packlist(root, &manifest).unwrap();

    // The last package within the cap (depth 32) is processed and ships.
    assert!(
        out.contains(&format!("node_modules/p{}/index.js", FIRST_REFUSED - 1)),
        "packages within MAX_BUNDLE_DEPTH must ship: {out:?}",
    );
    // `p33` is created at depth 33 (> 32); the cap refuses to descend,
    // so it and everything past it are dropped even though they exist
    // on disk.
    assert!(
        !out.iter().any(|p| p.starts_with(&format!("node_modules/p{FIRST_REFUSED}/"))),
        "packages past MAX_BUNDLE_DEPTH must be refused: {out:?}",
    );
}
