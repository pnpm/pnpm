use super::{
    AppState, Bytes, CACHE_BUDGET, CanonicalPackageName, DistView, HeaderMap, PackumentProcessing,
    RenderedPackument, Representation, Utc, Value, cache_key, next_precision_change, render,
    render_cached, render_document,
};
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tempfile::TempDir;

fn state(tmp: &TempDir) -> AppState {
    let config = Config::static_serve("127.0.0.1:0".parse().unwrap(), tmp.path().to_path_buf());
    AppState {
        inner: Arc::new(
            crate::server::state::AppInner::new(config, pnpr_auth::AuthState::in_memory(), None)
                .unwrap(),
        ),
    }
}

fn view() -> DistView<'static> {
    DistView { revision_registry: None, tar_integrities: None, tar_registry: false }
}

fn document() -> Vec<u8> {
    let integrity = ssri::Integrity::from(b"example tgz".as_slice()).to_string();
    serde_json::to_vec(&serde_json::json!({
        "name": "example-widget", "dist-tags": {"latest": "1.0.0"},
        "versions": {"1.0.0": {"name": "example-widget", "version": "1.0.0",
            "dist": {"integrity": integrity, "tarball": "https://upstream.example/example-widget/-/example-widget-1.0.0.tgz"}}}
    })).unwrap()
}

#[tokio::test]
async fn reuses_each_representation_and_invalidates_changed_inputs() {
    let tmp = TempDir::new().unwrap();
    let state = state(&tmp);
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    let base = "https://registry.example";
    for representation in
        [Representation::Full, Representation::Abbreviated, Representation::InstallV2]
    {
        let first =
            render_cached(&name, document().into(), base, view(), &state, representation).unwrap();
        let repeated =
            render_cached(&name, document().into(), base, view(), &state, representation).unwrap();
        assert!(Arc::ptr_eq(&first, &repeated));
        let other_base = render_cached(
            &name,
            document().into(),
            "https://other.example",
            view(),
            &state,
            representation,
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&first, &other_base));
        if representation != Representation::InstallV2 {
            assert_ne!(first.etag, other_base.etag);
        }
        let mut changed: Value = serde_json::from_slice(&document()).unwrap();
        changed["versions"]["1.0.0"]["deprecated"] = "use another version".into();
        let changed = render_cached(
            &name,
            serde_json::to_vec(&changed).unwrap().into(),
            base,
            view(),
            &state,
            representation,
        )
        .unwrap();
        assert_ne!(first.etag, changed.etag);
    }
    let full = render_cached(&name, document().into(), base, view(), &state, Representation::Full)
        .unwrap();
    let abbreviated =
        render_cached(&name, document().into(), base, view(), &state, Representation::Abbreviated)
            .unwrap();
    assert!(!Arc::ptr_eq(&full, &abbreviated));
    let table = Arc::new(BTreeMap::from([(
        ssri::Integrity::from(b"example tgz".as_slice()).to_string(),
        pnpr_storage::DerivedTar::new(
            ssri::Integrity::from(b"example tar".as_slice()).to_string(),
            Some(false),
            Some(2),
            Some(200),
        ),
    )]));
    let changed_view = DistView { tar_integrities: Some(&table), ..view() };
    let changed = render_cached(
        &name,
        document().into(),
        base,
        changed_view,
        &state,
        Representation::InstallV2,
    )
    .unwrap();
    let original =
        render_cached(&name, document().into(), base, view(), &state, Representation::InstallV2)
            .unwrap();
    assert_ne!(changed.etag, original.etag);
    let changed_doc: Value = serde_json::from_slice(&changed.body).unwrap();
    assert_eq!(changed_doc["versions"]["1.0.0"]["fileCount"], 2);
}

#[test]
fn abbreviation_expires_when_a_timestamp_crosses_the_precision_horizon() {
    let now = Utc::now();
    let crossing = now + chrono::Duration::minutes(1);
    let timestamp = crossing - chrono::Duration::days(pnpr_upstream::TIME_PRECISION_HORIZON_DAYS);
    let doc = serde_json::json!({"time": {"1.0.0": timestamp.to_rfc3339(), "invalid": "other"}});
    assert_eq!(next_precision_change(&doc, now), Some(crossing));
    assert_eq!(next_precision_change(&doc, crossing + chrono::Duration::nanoseconds(1)), None);
}

#[tokio::test(flavor = "current_thread")]
async fn large_packument_render_leaves_the_request_thread_available() {
    let tmp = TempDir::new().unwrap();
    let state = state(&tmp);
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    let mut doc: Value = serde_json::from_slice(&document()).unwrap();
    doc["readme"] = "x".repeat(16 * 1024 * 1024).into();
    let bytes = serde_json::to_vec(&doc).unwrap();
    let headers = HeaderMap::new();
    let done = AtomicBool::new(false);
    let started = Instant::now();
    let rendering = async {
        render(&name, bytes.clone().into(), "https://registry.example", view(), &state, &headers)
            .await
            .unwrap();
        done.store(true, Ordering::SeqCst);
    };
    let heartbeat = async {
        let mut yields = 0;
        while !done.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
            yields += 1;
        }
        yields
    };
    let ((), yields) = tokio::join!(rendering, heartbeat);
    let cold = started.elapsed();
    assert!(yields > 1, "render monopolized the request thread");
    let started = Instant::now();
    render(&name, bytes.into(), "https://registry.example", view(), &state, &headers)
        .await
        .unwrap();
    eprintln!(
        "16 MiB packument: cold {cold:?}, warm {:?}, async yields {yields}",
        started.elapsed(),
    );
}

#[tokio::test]
async fn cancellation_keeps_the_worker_permit_until_blocking_work_finishes() {
    let processing = Arc::new(PackumentProcessing::default());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = Arc::clone(&processing);
    let request = tokio::spawn(async move {
        worker
            .run(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .await
            .unwrap();
    });
    started_rx.await.unwrap();
    request.abort();
    let _ = request.await;
    assert_eq!(processing.workers.available_permits(), 1);
    release_tx.send(()).unwrap();
    processing.run(|| ()).await.unwrap();
}

#[tokio::test]
async fn expired_abbreviated_bytes_are_rendered_again() {
    let tmp = TempDir::new().unwrap();
    let state = state(&tmp);
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    let base = "https://registry.example";
    let representation = Representation::Abbreviated;
    let now = Utc::now();
    let stale = Arc::new(RenderedPackument {
        source: document().into(),
        body: Bytes::from_static(b"stale"),
        content_type: super::super::super::ABBREVIATED_CONTENT_TYPE,
        etag: "stale".to_string(),
        last_modified: None,
        rendered_at: now - chrono::Duration::minutes(2),
        expires: Some(now - chrono::Duration::minutes(1)),
    });
    let key = cache_key(&name, &document(), base, view(), representation, &state.inner.packuments)
        .unwrap();
    state.inner.packuments.cache
        .lock()
        .unwrap()
        .insert(key, Arc::clone(&stale), 5, CACHE_BUDGET);
    let current =
        render_cached(&name, document().into(), base, view(), &state, representation).unwrap();
    assert!(!Arc::ptr_eq(&stale, &current));
    let doc: Value = serde_json::from_slice(&current.body).unwrap();
    assert_eq!(doc["name"], "example-widget");
}

#[tokio::test]
#[ignore = "manual packument rendering measurement"]
async fn measure_large_packument_rendering() {
    let tmp = TempDir::new().unwrap();
    let state = state(&tmp);
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    let mut doc: Value = serde_json::from_slice(&document()).unwrap();
    let mut manifest = doc["versions"]["1.0.0"].clone();
    manifest["readme"] = "x".repeat(22 * 1024).into();
    for index in 0..2000 {
        let version = format!("1.{index}.0");
        manifest["version"] = version.clone().into();
        doc["versions"][&version] = manifest.clone();
    }
    let bytes = Bytes::from(serde_json::to_vec(&doc).unwrap());
    drop(doc);
    for (representation, accept) in [
        (Representation::Full, "application/json"),
        (Representation::Abbreviated, super::super::super::ABBREVIATED_CONTENT_TYPE),
        (Representation::InstallV2, super::super::super::split_packuments::INSTALL_V2_CONTENT_TYPE),
    ] {
        let headers = HeaderMap::from_iter([(axum::http::header::ACCEPT, accept.parse().unwrap())]);
        let mut uncached = Vec::new();
        let mut cached = Vec::new();
        render(&name, bytes.clone(), "https://registry.example", view(), &state, &headers)
            .await
            .unwrap();
        for _ in 0..3 {
            let start = Instant::now();
            let rendered = render_document(
                &name,
                bytes.clone(),
                "https://registry.example",
                view(),
                &state,
                representation,
                Utc::now(),
            )
            .unwrap();
            uncached.push(start.elapsed());
            drop(rendered);
            let fresh = Bytes::copy_from_slice(&bytes);
            let start = Instant::now();
            render(&name, fresh, "https://registry.example", view(), &state, &headers)
                .await
                .unwrap();
            cached.push(start.elapsed());
        }
        eprintln!(
            "{representation:?}, {} bytes / 2000 versions: uncached {uncached:?}; cached {cached:?}",
            bytes.len(),
        );
    }
}

#[tokio::test]
async fn fingerprint_collision_cannot_reuse_another_document() {
    let tmp = TempDir::new().unwrap();
    let state = state(&tmp);
    let name =
        CanonicalPackageName::parse("example-widget", pnpr_package_name::Ecosystem::Npm).unwrap();
    let base = "https://registry.example";
    let representation = Representation::Abbreviated;
    let original =
        render_cached(&name, document().into(), base, view(), &state, representation).unwrap();
    let mut changed: Value = serde_json::from_slice(&document()).unwrap();
    changed["versions"]["1.0.0"]["deprecated"] = "use another version".into();
    let bytes = Bytes::from(serde_json::to_vec(&changed).unwrap());
    let key =
        cache_key(&name, &bytes, base, view(), representation, &state.inner.packuments).unwrap();
    state.inner.packuments.cache
        .lock()
        .unwrap()
        .insert(
            key,
            Arc::clone(&original),
            original.source.len() + original.body.len(),
            CACHE_BUDGET,
        );
    let current = render_cached(&name, bytes, base, view(), &state, representation).unwrap();
    assert!(!Arc::ptr_eq(&original, &current));
    let current: Value = serde_json::from_slice(&current.body).unwrap();
    assert_eq!(current["versions"]["1.0.0"]["deprecated"], "use another version");
}
