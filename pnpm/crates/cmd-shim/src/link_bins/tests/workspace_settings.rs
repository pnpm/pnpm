use super::{
    Arc, BinOrigin, Host, LinkBinsOptions, PackageBinSource, Value, create_dir_all, json,
    link_bins_of_packages, read_file, read_to_string, tempdir, write_file,
};

#[test]
fn direct_origin_wins_over_hoisted_regardless_of_lexical() {
    let tmp = tempdir().unwrap();
    // Hoisted's package name `alpha` is lexically smaller than
    // direct's `zeta`, so the lexical-only rule would pick alpha.
    // The Direct/Hoisted tier must override that.
    let hoisted = tmp.path().join("alpha");
    let direct = tmp.path().join("zeta");
    for d in [&hoisted, &direct] {
        create_dir_all(d).unwrap();
        write_file(d.join("cmd.js"), "#!/usr/bin/env node\n").unwrap();
    }
    write_file(
        hoisted.join("package.json"),
        json!({"name": "alpha", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();
    write_file(
        direct.join("package.json"),
        json!({"name": "zeta", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();

    let manifest_hoisted: Value =
        serde_json::from_slice(&read_file(hoisted.join("package.json")).unwrap()).unwrap();
    let manifest_direct: Value =
        serde_json::from_slice(&read_file(direct.join("package.json")).unwrap()).unwrap();

    let bins = tmp.path().join(".bin");
    link_bins_of_packages::<Host>(
        &[
            PackageBinSource::new(hoisted, Arc::new(manifest_hoisted))
                .with_origin(BinOrigin::Hoisted),
            PackageBinSource::new(direct, Arc::new(manifest_direct)).with_origin(BinOrigin::Direct),
        ],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins.join("shared")).unwrap();
    assert!(
        body.contains("/zeta/cmd.js"),
        "Direct origin must win over Hoisted regardless of lexical order, got body:\n{body}",
    );
}

#[test]
fn hoisted_origin_loses_to_existing_direct() {
    let tmp = tempdir().unwrap();
    // Direct's name is lexically larger than hoisted's; lexical
    // fallback would replace it with hoisted, but the origin tier
    // shuts that out.
    let direct = tmp.path().join("zeta");
    let hoisted = tmp.path().join("alpha");
    for d in [&direct, &hoisted] {
        create_dir_all(d).unwrap();
        write_file(d.join("cmd.js"), "#!/usr/bin/env node\n").unwrap();
    }
    write_file(
        direct.join("package.json"),
        json!({"name": "zeta", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();
    write_file(
        hoisted.join("package.json"),
        json!({"name": "alpha", "bin": {"shared": "cmd.js"}}).to_string(),
    )
    .unwrap();

    let manifest_direct: Value =
        serde_json::from_slice(&read_file(direct.join("package.json")).unwrap()).unwrap();
    let manifest_hoisted: Value =
        serde_json::from_slice(&read_file(hoisted.join("package.json")).unwrap()).unwrap();

    let bins = tmp.path().join(".bin");
    // Direct goes first so it's the incumbent when the Hoisted
    // candidate is processed second.
    link_bins_of_packages::<Host>(
        &[
            PackageBinSource::new(direct, Arc::new(manifest_direct)).with_origin(BinOrigin::Direct),
            PackageBinSource::new(hoisted, Arc::new(manifest_hoisted))
                .with_origin(BinOrigin::Hoisted),
        ],
        &bins,
        &LinkBinsOptions::default(),
    )
    .unwrap();

    let body = read_to_string(bins.join("shared")).unwrap();
    assert!(
        body.contains("/zeta/cmd.js"),
        "Direct incumbent must shut out Hoisted candidate, got body:\n{body}",
    );
}

#[test]
fn publicly_hoisted_bin_wins_over_auto_installed_peer_in_either_order() {
    let tmp = tempdir().unwrap();
    let hoisted = tmp.path().join("zeta");
    let peer = tmp.path().join("alpha");
    for dir in [&hoisted, &peer] {
        create_dir_all(dir).unwrap();
        write_file(dir.join("cmd.js"), "#!/usr/bin/env node\n").unwrap();
    }
    let hoisted_manifest = Arc::new(json!({"name": "zeta", "bin": {"shared": "cmd.js"}}));
    let peer_manifest = Arc::new(json!({"name": "alpha", "bin": {"shared": "cmd.js"}}));
    for reverse in [false, true] {
        let hoisted_source = PackageBinSource::new(hoisted.clone(), Arc::clone(&hoisted_manifest))
            .with_origin(BinOrigin::Hoisted);
        let peer_source = PackageBinSource::new(peer.clone(), Arc::clone(&peer_manifest))
            .with_origin(BinOrigin::Peer);
        let sources =
            if reverse { [peer_source, hoisted_source] } else { [hoisted_source, peer_source] };
        let bins = tmp
            .path()
            .join(format!("bins-{reverse}"));
        link_bins_of_packages::<Host>(&sources, &bins, &LinkBinsOptions::default()).unwrap();
        let shim = bins.join(if cfg!(windows) { "shared.cmd" } else { "shared" });
        let body = read_to_string(shim).unwrap();
        assert!(body.contains("zeta"), "public bin should win: {body}");
    }
}
