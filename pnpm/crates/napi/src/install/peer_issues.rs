use super::{
    Arc, EngineCallGuard, EngineMode, InstallOptions, PeerIssuesOptions, engine_call_lock, napi,
    run_install_inner,
};

#[napi(js_name = "getPeerDependencyIssues")]
pub async fn get_peer_dependency_issues(
    options: PeerIssuesOptions,
) -> napi::Result<serde_json::Value> {
    let _guard = engine_call_lock().lock().await;
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pnpm-napi-peer-issues".to_string())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_peer_issues_blocking(options));
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to spawn peer-issues thread: {error}"))
        })?;
    rx.await.map_err(|_| napi::Error::from_reason("peer-issues worker thread panicked"))?
}

/// Run a sink-driven `dry_run` resolve and serialize the per-importer issues
/// into the `PeerDependencyIssuesByProjects` wire shape, including the
/// `conflicts` / `intersections` derivation v11's `mergePeers` does.
fn run_peer_issues_blocking(options: PeerIssuesOptions) -> napi::Result<serde_json::Value> {
    // No log sink for this query — engine events would interleave with
    // the caller's own reporting for what is a silent resolution.
    let _sink_guard = EngineCallGuard::new(None);
    let install_options = peer_issues_install_options(options)?;

    let sink: pnpm_package_manager::PeerIssuesSink = Arc::default();
    run_install_inner(&install_options, None, EngineMode::PeerIssues(Arc::clone(&sink)))?;
    let issues_by_importer =
        std::mem::take(&mut *sink.lock().expect("peer-issues sink lock poisoned"));

    let mut result = serde_json::Map::new();
    for (importer_id, issues) in issues_by_importer {
        result.insert(importer_id, peer_issues_to_json(&issues));
    }
    Ok(serde_json::Value::Object(result))
}

pub(super) fn peer_issues_install_options(
    options: PeerIssuesOptions,
) -> napi::Result<InstallOptions> {
    Ok(InstallOptions {
        dir: options.dir,
        projects: options.projects,
        store_dir: options.store_dir,
        cache_dir: options.cache_dir,
        registries: options.registries,
        auth_header_by_uri: options.auth_header_by_uri,
        proxy_config: options.proxy_config,
        network_config: options.network_config,
        overrides: options.overrides,
        peers_suffix_max_length: checked_u32_option(
            options.peers_suffix_max_length,
            "peersSuffixMaxLength",
        )?,
        virtual_store_dir_max_length: checked_u32_option(
            options.virtual_store_dir_max_length,
            "virtualStoreDirMaxLength",
        )?,
        // Unlike install, this query must expose missing peers by default.
        auto_install_peers: Some(options.auto_install_peers.unwrap_or(false)),
        ..InstallOptions::default()
    })
}

fn checked_u32_option(value: Option<f64>, name: &str) -> napi::Result<Option<u32>> {
    value
        .map(|value| {
            if value.is_finite()
                && value.fract() == 0.0
                && (0.0..=f64::from(u32::MAX)).contains(&value)
            {
                Ok(value as u32)
            } else {
                Err(napi::Error::from_reason(format!(
                    "getPeerDependencyIssues: `{name}` must be an integer from 0 through {}",
                    u32::MAX,
                )))
            }
        })
        .transpose()
}

/// Serialize one importer's issues into v11's `PeerDependencyIssues`
/// wire shape, deriving `conflicts` / `intersections` from the missing
/// peers the way v11's `mergePeers` does: all-optional names are
/// skipped, a single range passes through verbatim, and multiple
/// ranges intersect via semver bound-set intersection (`null`
/// intersection → conflict).
pub(super) fn peer_issues_to_json(
    issues: &pnpm_resolving_deps_resolver::PeerDependencyIssues,
) -> serde_json::Value {
    let missing = peer_missing_json(issues);
    let bad = peer_bad_json(issues);
    let (conflicts, intersections) = peer_intersections_json(issues);

    serde_json::json!({
        "missing": missing,
        "bad": bad,
        "conflicts": conflicts,
        "intersections": intersections,
    })
}

fn parents_json(parents: &pnpm_resolving_deps_resolver::ParentChain) -> Vec<serde_json::Value> {
    parents
        .to_refs()
        .into_iter()
        .map(|parent| serde_json::json!({ "name": parent.name, "version": parent.version }))
        .collect::<Vec<_>>()
}

fn peer_missing_json(
    issues: &pnpm_resolving_deps_resolver::PeerDependencyIssues,
) -> serde_json::Map<String, serde_json::Value> {
    let mut missing = serde_json::Map::new();
    for (peer_name, entries) in &issues.missing {
        missing.insert(
            peer_name.clone(),
            entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "parents": parents_json(&entry.parents),
                        "optional": entry.optional,
                        "wantedRange": entry.wanted_range,
                    })
                })
                .collect(),
        );
    }

    missing
}

fn peer_bad_json(
    issues: &pnpm_resolving_deps_resolver::PeerDependencyIssues,
) -> serde_json::Map<String, serde_json::Value> {
    let mut bad = serde_json::Map::new();
    for (peer_name, entries) in &issues.bad {
        bad.insert(
            peer_name.clone(),
            entries
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "parents": parents_json(&entry.parents),
                        "foundVersion": entry.found_version,
                        "resolvedFrom": parents_json(&entry.resolved_from),
                        "optional": entry.optional,
                        "wantedRange": entry.wanted_range,
                    })
                })
                .collect(),
        );
    }

    bad
}

fn peer_intersections_json(
    issues: &pnpm_resolving_deps_resolver::PeerDependencyIssues,
) -> (Vec<String>, serde_json::Map<String, serde_json::Value>) {
    let mut conflicts: Vec<String> = Vec::new();
    let mut intersections = serde_json::Map::new();
    for (peer_name, entries) in &issues.missing {
        if entries.iter().all(|entry| entry.optional) {
            continue;
        }
        if let [entry] = entries.as_slice() {
            intersections
                .insert(peer_name.clone(), serde_json::Value::String(entry.wanted_range.clone()));
            continue;
        }
        match safe_intersect(entries.iter().map(|entry| entry.wanted_range.as_str())) {
            Some(intersection) => {
                intersections.insert(peer_name.clone(), serde_json::Value::String(intersection));
            }
            None => conflicts.push(peer_name.clone()),
        }
    }
    conflicts.sort_unstable();
    (conflicts, intersections)
}

/// Intersect semver ranges pairwise. `None` when any range fails to
/// parse or the intersection is empty — the caller records a conflict,
/// matching v11's `safeIntersect` (which swallows
/// `semver-range-intersect` errors the same way).
pub(super) fn safe_intersect<'a>(ranges: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut acc: Option<node_semver::Range> = None;
    for range in ranges {
        let parsed: node_semver::Range = range.parse().ok()?;
        acc = Some(match acc {
            None => parsed,
            Some(current) => current.intersect(&parsed)?,
        });
    }
    acc.map(|range| range.to_string())
}
