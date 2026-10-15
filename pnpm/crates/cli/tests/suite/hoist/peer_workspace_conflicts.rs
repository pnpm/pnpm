use super::{ManifestDeps, WorkspaceFixture, fs, is_symlink_or_junction};

#[test]
fn auto_installed_peer_does_not_displace_workspace_hoist() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("linkWorkspacePackages: false\n");
    fixture.write_root_manifest("root", ManifestDeps::default());
    let foo = fixture.project("foo", "@pnpm.e2e/foo", ManifestDeps::default());
    fs::write(
        foo.join("package.json"),
        serde_json::json!({ "name": "@pnpm.e2e/foo", "version": "100.0.0" }).to_string(),
    )
    .expect("write workspace foo manifest");
    fixture.project(
        "workspace-consumer",
        "workspace-consumer",
        ManifestDeps { prod: &[("@pnpm.e2e/foo", "workspace:*")], ..Default::default() },
    );
    let peer_consumer_project = fixture.project(
        "peer-consumer",
        "peer-consumer",
        ManifestDeps { prod: &[("@pnpm.e2e/has-foo100-peer", "1.0.0")], ..Default::default() },
    );

    let assert_layout = || {
        let hoisted = fixture.workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foo");
        assert!(is_symlink_or_junction(&hoisted).unwrap(), "workspace hoist must exist");
        assert_eq!(fs::canonicalize(&hoisted).unwrap(), fs::canonicalize(&foo).unwrap());

        let peer_consumer_package =
            fs::canonicalize(peer_consumer_project.join("node_modules/@pnpm.e2e/has-foo100-peer"))
                .expect("resolve package with peer");
        let installed_peer = peer_consumer_package
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("@pnpm.e2e/foo");
        assert!(installed_peer.exists(), "auto-installed peer must be beside its consumer");
        assert_ne!(fs::canonicalize(installed_peer).unwrap(), fs::canonicalize(&foo).unwrap());
    };

    fixture.run(["install"]);
    assert_layout();
    fixture.run(["dedupe"]);
    assert_layout();
    fixture.run(["install", "--frozen-lockfile"]);
    assert_layout();
}
