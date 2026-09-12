use super::{
    NetworkConfigInput, NoProxySetting, NodeApiProject, PeerIssuesOptions, ProxyConfigInput,
    build_overlay, network_config, peer_issues_install_options, peer_issues_options,
};

#[test]
fn peer_issues_options_preserve_shared_engine_options() {
    let install_options = peer_issues_install_options(PeerIssuesOptions {
        dir: "/project".to_string(),
        projects: vec![NodeApiProject {
            root_dir: "/project".to_string(),
            manifest: serde_json::json!({ "name": "project" }),
            dependency_manifest: None,
        }],
        store_dir: Some("/store".to_string()),
        cache_dir: Some("/cache".to_string()),
        registries: Some(
            [("default".to_string(), "https://registry.example.com".to_string())].into(),
        ),
        auth_header_by_uri: Some(
            [("//registry.example.com/".to_string(), "Bearer token".to_string())].into(),
        ),
        proxy_config: Some(ProxyConfigInput {
            http_proxy: Some("http://proxy.example.com".to_string()),
            https_proxy: Some("https://proxy.example.com".to_string()),
            no_proxy: Some(serde_json::json!(true)),
        }),
        network_config: Some(NetworkConfigInput {
            ca: Some(serde_json::json!("certificate")),
            strict_ssl: Some(false),
            network_concurrency: Some(12),
            ..network_config()
        }),
        overrides: Some([("foo".to_string(), "1.0.0".to_string())].into()),
        peers_suffix_max_length: Some(1_000.0),
        virtual_store_dir_max_length: Some(120.0),
        auto_install_peers: Some(true),
    })
    .expect("valid peer issues options");

    assert_eq!(install_options.dir, "/project");
    assert_eq!(install_options.projects.len(), 1);
    assert_eq!(install_options.store_dir.as_deref(), Some("/store"));
    assert_eq!(install_options.cache_dir.as_deref(), Some("/cache"));
    assert_eq!(
        install_options.registries.as_ref().unwrap()["default"],
        "https://registry.example.com",
    );
    assert_eq!(
        install_options.auth_header_by_uri.as_ref().unwrap()["//registry.example.com/"],
        "Bearer token",
    );
    assert_eq!(install_options.overrides.as_ref().unwrap()["foo"], "1.0.0");
    assert_eq!(install_options.peers_suffix_max_length, Some(1_000));
    assert_eq!(install_options.virtual_store_dir_max_length, Some(120));
    assert_eq!(install_options.auto_install_peers, Some(true));

    let overlay = build_overlay(&install_options, false).expect("overlay");
    assert_eq!(overlay.network_concurrency, Some(12));
    assert_eq!(overlay.proxy.unwrap().no_proxy, Some(NoProxySetting::Bypass));
    let tls = overlay.tls.expect("tls");
    assert_eq!(tls.ca, vec!["certificate".to_string()]);
    assert_eq!(tls.strict_ssl, Some(false));
}

#[test]
fn peer_issues_options_disable_auto_install_peers_by_default() {
    assert_eq!(
        peer_issues_install_options(peer_issues_options())
            .expect("valid peer issues options")
            .auto_install_peers,
        Some(false),
    );
}

#[test]
fn peer_issues_options_reject_invalid_u32_values() {
    for value in [-1.0, 1.5, f64::from(u32::MAX) + 1.0, f64::INFINITY, f64::NAN] {
        let mut options = peer_issues_options();
        options.peers_suffix_max_length = Some(value);
        assert!(
            peer_issues_install_options(options).is_err(),
            "peersSuffixMaxLength accepted {value:?}",
        );

        let mut options = peer_issues_options();
        options.virtual_store_dir_max_length = Some(value);
        assert!(
            peer_issues_install_options(options).is_err(),
            "virtualStoreDirMaxLength accepted {value:?}",
        );
    }
}

/// `safe_intersect` mirrors v11's `mergePeers` helper: pairwise semver
/// range intersection, `None` on an empty intersection or unparsable
/// range (→ recorded as a conflict by the caller).
#[test]
fn safe_intersect_matches_merge_peers_semantics() {
    use super::super::peer_issues::safe_intersect;

    // Overlapping ranges intersect to a non-empty range.
    let merged = safe_intersect(["^16.8.0", "16 || 17"].into_iter()).expect("ranges overlap");
    let range: node_semver::Range = merged.parse().expect("intersection parses");
    assert!(range.satisfies(&"16.9.1".parse().unwrap()));
    assert!(!range.satisfies(&"17.0.0".parse().unwrap()));

    // Disjoint ranges → None (conflict).
    assert_eq!(safe_intersect(["^16.0.0", "^17.0.0"].into_iter()), None);
    // Unparsable range → None, matching v11's swallow-errors behavior.
    assert_eq!(safe_intersect(["^16.0.0", "not-a-range"].into_iter()), None);
    // A version its partner excludes drops out whichever side it is passed on.
    assert_eq!(safe_intersect(["<1.2.3", "1.2.3"].into_iter()), None);
    assert_eq!(safe_intersect(["1.2.3", "<1.2.3"].into_iter()), None);
    // An upper bound that leaves a component out reaches the whole line.
    let merged = safe_intersect(["<=16", "^16.8.0"].into_iter()).expect("ranges overlap");
    let range: node_semver::Range = merged.parse().expect("intersection parses");
    assert!(range.satisfies(&"16.9.1".parse().unwrap()));
}

/// The wire shape mirrors v11's `PeerDependencyIssues`: `missing` /
/// `bad` entries verbatim, `intersections` from the non-optional
/// missing ranges, and disjoint ranges surfacing under `conflicts`.
#[test]
fn peer_issues_to_json_derives_conflicts_and_intersections() {
    use pnpm_resolving_deps_resolver::{
        MissingPeer, ParentChain, PeerDependencyIssue, PeerDependencyIssues,
    };

    let missing_entry = |range: &str, optional: bool| MissingPeer {
        wanted_range: range.to_string(),
        raw_range: range.to_string(),
        optional,
        parents: ParentChain::from_names(["comp1".to_string()]),
    };
    let mut issues = PeerDependencyIssues::default();
    issues.missing.insert("react".to_string(), vec![missing_entry("^16.8.0", false)]);
    issues.missing.insert(
        "conflicted".to_string(),
        vec![missing_entry("^1.0.0", false), missing_entry("^2.0.0", false)],
    );
    issues.missing.insert("optional-only".to_string(), vec![missing_entry("*", true)]);
    issues.bad.insert(
        "styled".to_string(),
        vec![PeerDependencyIssue {
            wanted_range: "^5.0.0".to_string(),
            found_version: "4.1.0".to_string(),
            optional: false,
            parents: ParentChain::from_names([
                "root".to_string(),
                "mid".to_string(),
                "leaf".to_string(),
            ]),
            resolved_from: ParentChain::from_names(["provider".to_string()]),
        }],
    );

    let json = super::super::peer_issues::peer_issues_to_json(&issues);
    assert_eq!(json["intersections"]["react"], "^16.8.0");
    assert_eq!(json["conflicts"], serde_json::json!(["conflicted"]));
    assert!(json["intersections"].get("optional-only").is_none());
    assert_eq!(json["missing"]["react"][0]["wantedRange"], "^16.8.0");
    assert_eq!(json["missing"]["react"][0]["parents"][0]["name"], "comp1");
    assert_eq!(
        json["bad"]["styled"][0]["parents"],
        serde_json::json!([
            { "name": "root", "version": "" },
            { "name": "mid", "version": "" },
            { "name": "leaf", "version": "" },
        ]),
    );
    assert_eq!(
        json["bad"]["styled"][0]["resolvedFrom"],
        serde_json::json!([{ "name": "provider", "version": "" }]),
    );
    assert_eq!(json["bad"]["styled"][0]["foundVersion"], "4.1.0");
}
