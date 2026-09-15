use super::{
    HashMap, HashSet, alias_dependency, assert_eq, importer_locked_peer_versions,
    peer_context_lockfile, peer_declaring_metadata, snapshot_with_dependencies,
};

/// Nothing ranks two ordinary aliases onto one provider, and guessing
/// between them would vary with map order.
#[test]
fn competing_ordinary_aliases_do_not_rename_the_segment() {
    let lockfile = peer_context_lockfile(
        Some(("consumer@1.0.0", peer_declaring_metadata([]))),
        [(
            "consumer@1.0.0(alias-provider@1.0.0)",
            snapshot_with_dependencies([
                ("one", alias_dependency("alias-provider@1.0.0")),
                ("other", alias_dependency("alias-provider@1.0.0")),
            ]),
        )],
    );

    let versions = importer_locked_peer_versions(Some(&lockfile), "missing-importer");
    assert_eq!(
        versions,
        HashMap::from_iter([(
            "alias-provider".to_string(),
            HashSet::from_iter(["1.0.0".to_string()]),
        )]),
    );
}
