use std::collections::HashSet;

use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpm_crypto_hash::{create_hex_hash, integrity_addressed_tarball_path};
use serde_json::{Value, json};
use ssri::Integrity;

use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_policy::Identity;
use pnpr_registry::Registry;
use pnpr_storage::{
    HostedPackumentVersion, PackumentWrite, TarballFinalize,
    journal::{JournaledPublish, JournaledRevisionRef},
    publish::{
        PendingAttachment, extract_attachments, merge_manifest, now_iso,
        stream_decode_verify_and_write,
    },
};

use super::{
    Action, AppState, AuthedCaller, HostedGate, HostedOriginalRef, RegistrySource, WriteTarget,
    authorize, authorized_upstream, default_registry_target, hosted_gate, hosted_storage,
    resolve_registry_source, resolve_write_target,
};

/// Where a publish of `package` writes, given an optional explicit `/~<name>/`.
pub(super) enum PublishTarget {
    /// Write into the hosted registry `source`'s storage namespace `org`.
    /// The source name is carried so the write's `publish`/`unpublish`
    /// authorization can consult that registry's `packages:` rules.
    Hosted { source: String, org: String },
    /// The resolved target is not a hosted org; reject with this reason.
    Reject(String),
    /// The resolved upstream registry denies this caller; answer with the
    /// same response its reads give (a 403), before any rejection that would
    /// narrate routing config.
    Denied(RegistryError),
    /// The addressed registry or route does not exist (or the path-less base has
    /// no default target).
    NotFound,
}

/// Resolve where a publish lands. A write may only target a hosted registry
/// whose declared patterns claim the name: a selection of an upstream is
/// rejected ("name a hosted registry"), never silently landing on an upstream,
/// and an unclaimed name is rejected with the reason — so a typo'd scope
/// fails loudly at publish time instead of storing a name the registry's
/// namespace can never serve. The registry's `access` list gates the write
/// exactly as it gates reads — a caller the registry denies gets the same
/// not-found mask as on a read, whether the name is claimed or not
/// ([`registry_visible_to_caller`] gates the loud rejection), so a private
/// registry neither accepts the write nor reveals that it exists. The
/// path-less base routes through its default-target registry; with no default
/// target the bare host has no registry and the publish is a not-found,
/// exactly like a read.
pub(super) fn resolve_publish_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    package: &str,
) -> PublishTarget {
    let (target, context) = match registry {
        Some(registry) => (registry.to_string(), format!("through registry {registry:?}")),
        None => match default_registry_target(state) {
            Some(target) => (target, "to the path-less base".to_string()),
            None => return PublishTarget::NotFound,
        },
    };
    match resolve_registry_source(state, &target, package) {
        RegistrySource::Hosted(registry) => {
            match hosted_gate(state, identity, &registry, package) {
                HostedGate::Allowed(org) => PublishTarget::Hosted { source: registry, org },
                HostedGate::MaskNotFound => PublishTarget::NotFound,
                HostedGate::Denied(err) => PublishTarget::Denied(err),
            }
        }
        // A write can never land on an upstream — but the upstream's `access:`
        // gates the write endpoints exactly as it gates reads, so a caller the
        // upstream denies gets the read path's 403 (`authorized_upstream`), not
        // a rejection that narrates where the name routes.
        RegistrySource::Upstream(source) => match authorized_upstream(state, identity, &source) {
            Err(response) => PublishTarget::Denied(response),
            Ok(_) => PublishTarget::Reject(format!(
                "cannot publish {package:?} {context}: it routes to an upstream registry; name \
                 a hosted registry",
            )),
        },
        // The loud rejection explains a config fact about the addressed
        // registry, so only a caller the registry is visible to gets it;
        // anyone else keeps the same not-found mask a read gives, so an
        // off-pattern probe cannot distinguish a private registry from an
        // undefined one.
        RegistrySource::Unclaimed => {
            if registry_visible_to_caller(state, identity, &target) {
                PublishTarget::Reject(format!(
                    "cannot publish {package:?} {context}: no registry's declared `patterns:` \
                     claim this package name",
                ))
            } else {
                PublishTarget::NotFound
            }
        }
        RegistrySource::NotFound => PublishTarget::NotFound,
    }
}

/// Whether `identity` may learn that the registry `name` exists. A hosted
/// registry is masked behind its access list — a denied caller sees the same
/// not-found as for an undefined name on every read, so nothing on the write
/// path may answer differently. An upstream registry is not masked (a denied
/// caller gets an explicit 403 on reads), and a router is visible whenever
/// any of its sources is.
pub(super) fn registry_visible_to_caller(
    state: &AppState,
    identity: &Identity,
    name: &str,
) -> bool {
    let concrete_visible = |name: &str| match state.inner.config.registries.get(name) {
        // The name being probed is unclaimed, so there is no per-package
        // entry to consult: the registry-level default `access:` decides
        // whether the caller may learn the registry exists at all.
        Some(Registry::Hosted { .. }) => state
            .inner
            .config
            .hosted
            .get(name)
            .is_some_and(|hosted| hosted.rules.default_access().allows(identity)),
        Some(Registry::Upstream { .. }) => true,
        Some(Registry::Router { .. }) | None => false,
    };
    match state.inner.config.registries.get(name) {
        Some(Registry::Router { sources }) => sources.iter().any(|source| concrete_visible(source)),
        Some(_) => concrete_visible(name),
        None => false,
    }
}

/// `PUT /:pkg` (path-less) or `PUT /~<name>/:pkg` — publish a new version (or
/// republish). Body is the full packument with `_attachments` carrying the
/// tarball bytes base64-encoded.
pub(super) async fn publish_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    body: axum::body::Bytes,
) -> Response {
    let name = match PackageName::parse(raw_name) {
        Ok(n) => n,
        Err(err) => return err.into_response(),
    };

    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };

    // Reject a publish whose body name disagrees with the URL.
    // npm/verdaccio return 400 here too; without this check a
    // misrouted PUT silently overwrites the wrong on-disk
    // package.json with another package's manifest.
    let body_name = incoming.get("name").and_then(Value::as_str);
    if body_name.is_some_and(|body_name| body_name != name.as_str()) {
        return RegistryError::BadRequest {
            reason: format!(
                "package in URL ({:?}) does not match body ({:?})",
                name.as_str(),
                body_name.unwrap_or(""),
            ),
        }
        .into_response();
    }

    // Routing, masking, and the publish rule all run inside
    // `validate_publish_doc`: the write resolves to a hosted registry (or
    // fails closed), and that registry's `packages:` rules authorize it.
    let (validated, target) =
        match validate_publish_doc(state, identity, registry, name, incoming).await {
            Ok(validated) => validated,
            Err(err) => return err.into_response(),
        };

    // Serialize the read-merge-write against other writers of this same
    // package on this instance, so a concurrent publish can't read the
    // same `existing`, merge a different version, and overwrite ours.
    // Held until this function returns, past the packument write below.
    let _packument_guard = state.inner.package_locks.lock(validated.name.as_str()).await;

    let staged = match stage_publish(state, validated, &now_iso(), Some(&target.org)).await {
        Ok(staged) => staged,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = commit_publishes(state, vec![staged]).await {
        return err.into_response();
    }
    publish_created_response()
}

/// `PUT /-/pnpm/v1/publish` — publish several packages with one
/// request. The body is `{"packages": [<publish doc>, ...]}` where
/// each entry is exactly the JSON body that `PUT /:pkg` takes
/// (packument with `_attachments`). `pnpm publish --batch` sends
/// this; the endpoint is not part of the standard npm registry API.
///
/// The batch is all-or-nothing up to the commit point: every
/// document is validated (name, publish policy, attachment
/// integrity) and every tarball of every package is fully written
/// to a tmp slot before anything becomes visible to readers, so a
/// batch that fails validation or staging leaves no new versions
/// behind.
pub(super) async fn serve_batch_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: axum::body::Bytes,
) -> Response {
    let incoming: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let Value::Object(mut incoming) = incoming else {
        return RegistryError::BadRequest { reason: "body must be a JSON object".to_string() }
            .into_response();
    };
    let Some(Value::Array(docs)) = incoming.remove("packages") else {
        return RegistryError::BadRequest {
            reason: "body must have a `packages` array".to_string(),
        }
        .into_response();
    };
    if docs.is_empty() {
        return RegistryError::BadRequest { reason: "`packages` must not be empty".to_string() }
            .into_response();
    }

    let mut validated = Vec::with_capacity(docs.len());
    let mut seen_names = std::collections::BTreeSet::new();
    for doc in docs {
        let Some(doc_name) = doc.get("name").and_then(Value::as_str) else {
            return RegistryError::BadRequest {
                reason: "every entry in `packages` must have a string `name`".to_string(),
            }
            .into_response();
        };
        let name = match PackageName::parse(doc_name) {
            Ok(name) => name,
            Err(err) => return err.into_response(),
        };
        // One packument read-merge-write per package: with the same
        // package twice in a batch, the second entry's merge would
        // depend on the first's uncommitted result. Senders carry
        // multiple versions of one package as several `versions`
        // entries in a single document instead.
        if !seen_names.insert(name.as_str().to_string()) {
            return RegistryError::BadRequest {
                reason: format!("duplicate package {:?} in `packages`", name.as_str()),
            }
            .into_response();
        }
        // The batch endpoint is path-less, so each package routes via the
        // default target; validation resolves that route and checks the
        // resolved hosted registry's publish rule per document.
        match validate_publish_doc(&state, &identity, None, name, doc).await {
            Ok(doc) => validated.push(doc),
            Err(err) => return err.into_response(),
        }
    }

    // Hold every affected package's lock across the whole
    // stage-and-commit, so concurrent writers of any package in the
    // batch serialize with us just like with a single publish.
    let names: Vec<&str> = validated.iter().map(|(doc, _)| doc.name.as_str()).collect();
    let _guards = state.inner.package_locks.lock_many(&names).await;

    let now = now_iso();
    let mut staged: Vec<StagedPublish> = Vec::with_capacity(validated.len());
    for (doc, target) in validated {
        // Each document's write target was resolved during validation, so a
        // routing failure surfaced before any tarball was staged.
        match stage_publish(&state, doc, &now, Some(&target.org)).await {
            Ok(stage) => staged.push(stage),
            Err(err) => {
                for stage in staged {
                    cleanup_tmp_slots(stage.slots).await;
                }
                return err.into_response();
            }
        }
    }
    if let Err(err) = commit_publishes(&state, staged).await {
        return err.into_response();
    }
    publish_created_response()
}

/// A publish document that passed every check that can run before
/// taking the package lock: the caller may publish the package, and
/// each attachment maps to a canonical disk filename and a
/// `versions[v].dist` block.
pub(super) struct ValidatedPublish {
    pub(super) name: PackageName,
    /// The publish body with `_attachments` stripped.
    pub(super) incoming: Value,
    /// One entry per attachment.
    pub(super) prepared: Vec<PreparedAttachment>,
}

/// One publish attachment resolved to its canonical on-disk filename and its
/// `versions[version].dist` block.
pub(super) struct PreparedAttachment {
    attachment: PendingAttachment,
    /// Canonical on-disk filename.
    canonical: String,
    /// The version this attachment publishes, parsed from its filename.
    /// Lets the re-publish guard tell a content publish from a metadata-only
    /// update (which carries no attachments).
    pub(super) version: String,
    /// The matching `dist` block, or `Value::Null` when absent.
    pub(super) dist: Value,
}

pub(super) async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: PackageName,
    mut incoming: Value,
) -> Result<(ValidatedPublish, WriteTarget), RegistryError> {
    // Route the write to its hosted registry first (masking a denied caller
    // as not-found, rejecting an upstream target), then check that
    // registry's `publish` rule for this package — so routing failures
    // surface before any 401/403 that would reveal a masked name exists.
    let target = resolve_write_target(state, identity, registry, &name)?;
    authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
    )?;

    let attachments = extract_attachments(&mut incoming)?;

    // Resolve each attachment's canonical disk filename + matching
    // `versions[v].dist` block. Attachment names that don't match the
    // package (`bar-1.0.0.tgz` for `foo`) or that try to escape the
    // package dir (`../../etc/passwd.tgz`) are rejected here, before
    // any I/O. The canonical name is what we actually persist — for
    // scoped libnpmpublish bodies the wire form is `@scope/name-version.tgz`
    // but on disk it lives at `<root>/@scope/name/name-version.tgz`,
    // matching what `serve_tarball` expects.
    let mut prepared: Vec<PreparedAttachment> = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let (canonical, version) = name.parse_tarball_name(&attachment.filename)?;
        let dist = incoming
            .get("versions")
            .and_then(|versions| versions.get(&version))
            .and_then(|manifest| manifest.get("dist"))
            .cloned()
            .unwrap_or(Value::Null);
        prepared.push(PreparedAttachment { attachment, canonical, version, dist });
    }
    Ok((ValidatedPublish { name, incoming, prepared }, target))
}

/// A publish whose packument is merged and whose tarballs are fully
/// written to tmp slots — everything verified, nothing visible to
/// readers yet. [`commit_publishes`] makes it visible.
pub(super) struct StagedPublish {
    name: PackageName,
    merged_bytes: Vec<u8>,
    base_version: Option<HostedPackumentVersion>,
    slots: Vec<pnpr_storage::TarballSlot>,
    original_refs: Vec<JournaledRevisionRef>,
    /// Hosted-org storage namespace this publish targets, or `None` for the
    /// flat (path-less) hosted store. Threaded into the commit and journal so
    /// the write — and any crash-recovery roll-forward — lands in the right org.
    org: Option<String>,
}

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`]. On error, every tmp file this call wrote is
/// removed.
pub(super) async fn stage_publish(
    state: &AppState,
    doc: ValidatedPublish,
    now_iso: &str,
    org: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let ValidatedPublish { name, incoming, prepared } = doc;
    let storage = hosted_storage(state, org);

    let hosted_packument = storage.read_hosted_packument_for_update(&name).await?;
    let (hosted_bytes, base_version) = match hosted_packument {
        Some(packument) => (Some(packument.bytes), Some(packument.version)),
        None => (None, None),
    };
    let hosted: Option<Value> = match hosted_bytes.as_deref().map(serde_json::from_slice) {
        Some(Ok(value)) => Some(value),
        Some(Err(err)) => return Err(RegistryError::Json(err)),
        None => None,
    };

    // Validate each incoming version against the locally hosted packument
    // (a hosted packument is served as-is, so anything not in it is genuinely
    // new here, even if it exists upstream):
    //
    // * Already hosted — published content is immutable, so reject a *content*
    //   re-publish with 409 (as npm/verdaccio do): one that carries a new
    //   tarball (an attachment) or changes `dist.integrity` (the content
    //   anchor; the `tarball` URL is rewritten on read, so don't compare it).
    //   A clash that does neither is a metadata-only update (`pnpm deprecate`),
    //   which is allowed — `merge_versions` keeps the hosted `dist`.
    // * New — it must ship a tarball. A version entry with no attachment would
    //   be advertised with no hosted tarball (installs 404) and would block a
    //   later real publish of it (409): reject with 400.
    let attachment_versions: HashSet<&str> =
        prepared.iter().map(|attachment| attachment.version.as_str()).collect();
    let hosted_versions =
        hosted.as_ref().and_then(|h| h.get("versions")).and_then(Value::as_object);
    if let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) {
        for (version, incoming_manifest) in incoming_versions {
            let has_attachment = attachment_versions.contains(version.as_str());
            match hosted_versions.and_then(|hosted| hosted.get(version)) {
                Some(hosted_manifest) => {
                    let incoming_integrity =
                        incoming_manifest.pointer("/dist/integrity").and_then(Value::as_str);
                    let hosted_integrity =
                        hosted_manifest.pointer("/dist/integrity").and_then(Value::as_str);
                    let integrity_changed = incoming_integrity
                        .is_some_and(|integrity| Some(integrity) != hosted_integrity);
                    if has_attachment || integrity_changed {
                        return Err(RegistryError::VersionAlreadyPublished {
                            package: name.as_str().to_string(),
                            version: version.clone(),
                        });
                    }
                }
                None if !has_attachment => {
                    return Err(RegistryError::BadRequest {
                        reason: format!(
                            "cannot publish version {version} of {:?} without a tarball",
                            name.as_str(),
                        ),
                    });
                }
                None => {}
            }
        }
    }

    // A hosted registry has no upstream, so a publish seeds the merge only from
    // the org's own hosted packument; a brand-new package starts from `None`.
    let existing: Option<Value> = hosted.clone();
    let merged = merge_manifest(existing.as_ref(), &incoming, hosted.as_ref(), now_iso);
    let merged_bytes = serde_json::to_vec_pretty(&merged).map_err(RegistryError::Json)?;
    let original_refs = prepared
        .iter()
        .filter_map(|attachment| staged_hosted_original_ref(&name, attachment))
        .collect();
    // `incoming` is no longer needed; drop it so the base64 strings
    // inside go away as soon as `prepared` (which owns each one) is
    // drained below.
    drop(incoming);

    // Stream-decode + verify + write each tarball. A mismatch — or a
    // missing integrity field — short-circuits the publish with a
    // 400; any tmp files written before the failure get removed
    // along the way so a bad upload leaves no on-disk artifact.
    let mut written_slots = Vec::with_capacity(prepared.len());
    for PreparedAttachment { attachment, canonical, version: _, dist } in prepared {
        let slot = match storage.reserve_hosted_tarball(&name, &canonical).await {
            Ok(slot) => slot,
            Err(err) => {
                cleanup_tmp_slots(written_slots).await;
                return Err(err);
            }
        };
        let PendingAttachment { filename, data, declared_length } = attachment;
        let tmp_path = slot.tmp_path.clone();
        let dist_for_task = (!dist.is_null()).then_some(dist);
        let result = tokio::task::spawn_blocking(move || {
            let dist_ref = dist_for_task.as_ref();
            stream_decode_verify_and_write(&filename, &data, declared_length, dist_ref, &tmp_path)
        })
        .await;
        match result {
            Ok(Ok(_)) => written_slots.push(slot),
            Ok(Err(err)) => {
                cleanup_tmp_slots(written_slots).await;
                return Err(err);
            }
            Err(join_err) => {
                let _ = tokio::fs::remove_file(&slot.tmp_path).await;
                cleanup_tmp_slots(written_slots).await;
                return Err(RegistryError::Io(std::io::Error::other(join_err.to_string())));
            }
        }
    }
    Ok(StagedPublish {
        name,
        merged_bytes,
        base_version,
        slots: written_slots,
        original_refs,
        org: org.map(str::to_string),
    })
}

fn staged_hosted_original_ref(
    package: &PackageName,
    attachment: &PreparedAttachment,
) -> Option<JournaledRevisionRef> {
    let integrity: Integrity = attachment.dist.get("integrity")?.as_str()?.parse().ok()?;
    let path = integrity_addressed_tarball_path(&integrity)?;
    let digest = path.strip_prefix("-/tarballs/sha512/")?.to_string();
    let record = HostedOriginalRef {
        package: package.as_str().to_string(),
        version: attachment.version.clone(),
    };
    let bytes = serde_json::to_vec(&record).expect("hosted original reference serializes");
    let ref_id = create_hex_hash(&format!("{}\0{}", record.package, record.version));
    Some(JournaledRevisionRef { filename: attachment.canonical.clone(), digest, ref_id, bytes })
}

/// Make every staged publish visible. The full intent — merged
/// packument bytes, revision references, and staged tmp-file locations — is sealed into
/// the commit journal first, so a crash or I/O failure mid-apply can
/// never leave the batch partially published: startup recovery rolls
/// a sealed transaction forward. If sealing itself fails, nothing was
/// promoted and the staged tmp files are cleaned up here.
///
/// Within each package, tarballs are promoted before the packument so
/// a successful packument write never advertises a tarball that's
/// missing from disk.
pub(super) async fn commit_publishes(
    state: &AppState,
    staged: Vec<StagedPublish>,
) -> Result<(), RegistryError> {
    let journal = state.inner.storage.publish_journal();
    let entries: Vec<JournaledPublish<'_>> = staged
        .iter()
        .map(|stage| JournaledPublish {
            name: &stage.name,
            org: stage.org.as_deref(),
            packument: &stage.merged_bytes,
            slots: &stage.slots,
            revision_refs: &stage.original_refs,
        })
        .collect();
    let sealed = journal.seal(&entries).await;
    drop(entries);
    let txn = match sealed {
        Ok(txn) => txn,
        Err(err) => {
            for stage in staged {
                cleanup_tmp_slots(stage.slots).await;
            }
            return Err(err);
        }
    };
    let revision_ref_owner = txn.revision_ref_owner().to_string();
    // Past the seal the transaction is committed: the apply below is pure
    // roll-forward, and failures must NOT clean up the staged files. If
    // the apply fails partway, complete it immediately via the same
    // idempotent recovery path so a running server never leaves the batch
    // partially visible; startup recovery is the final backstop if even
    // that fails.
    let apply_result = async {
        for stage in staged {
            // Promote into the package's hosted namespace (or the flat
            // store when it has none) — the same target the journal recorded,
            // so an inline failure and a startup roll-forward land identically.
            let store = hosted_storage(state, stage.org.as_deref());
            for slot in stage.slots {
                match store.finalize_tarball_slot(slot).await? {
                    TarballFinalize::Written | TarballFinalize::AlreadyIdentical => {}
                    // A concurrent replica already promoted a different tarball
                    // for this version. Its bytes are immutable, so abort the
                    // apply rather than advertise our integrity against them.
                    // The seal's roll-forward re-runs from the journal, where it
                    // drops the version we lost and re-merges the rest.
                    TarballFinalize::Conflict => {
                        return Err(RegistryError::PackumentWriteConflict {
                            package: stage.name.as_str().to_string(),
                        });
                    }
                }
            }
            for original in &stage.original_refs {
                store
                    .write_hosted_revision_ref(
                        &original.digest,
                        &original.ref_id,
                        &revision_ref_owner,
                        &original.bytes,
                    )
                    .await?;
            }
            match store
                .write_hosted_packument_if_current(
                    &stage.name,
                    &stage.merged_bytes,
                    stage.base_version.as_ref(),
                )
                .await?
            {
                PackumentWrite::Written => {
                    for original in &stage.original_refs {
                        store
                            .commit_hosted_revision_ref(
                                &original.digest,
                                &original.ref_id,
                                &revision_ref_owner,
                            )
                            .await?;
                    }
                }
                // Tarballs are already promoted at this point. A conflict means
                // another replica advanced the packument since staging, so the
                // base version is stale. Surfacing it drops into the seal's
                // roll-forward path (the caller), which re-reads the current
                // packument and re-merges this transaction's journaled manifest —
                // re-referencing the promoted tarballs — rather than leaving them
                // orphaned. Only if roll-forward and startup recovery both never
                // converge would a promoted tarball stay unreferenced.
                PackumentWrite::Conflict => {
                    return Err(RegistryError::PackumentWriteConflict {
                        package: stage.name.as_str().to_string(),
                    });
                }
            }
        }
        Ok::<(), RegistryError>(())
    }
    .await;
    match apply_result {
        Ok(()) => {
            txn.finish().await;
            Ok(())
        }
        Err(apply_err) => {
            tracing::warn!(error = %apply_err, "publish apply failed after seal; rolling forward");
            let report_conflict =
                matches!(&apply_err, RegistryError::RevisionReferenceLimit { .. });
            match txn.roll_forward(&state.inner.storage).await {
                Ok(()) if report_conflict => Err(apply_err),
                Ok(()) => Ok(()),
                Err(_) => Err(apply_err),
            }
        }
    }
}

fn publish_created_response() -> Response {
    let body = json!({ "ok": true, "success": true });
    let bytes = serde_json::to_vec(&body).expect("static-shape JSON serializes");
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Remove every tmp tarball file that a partially-completed publish
/// already wrote. Errors are swallowed: the caller is already
/// returning an error response, and a leftover `*.tmp.*` file is
/// harmless beyond a small amount of disk.
async fn cleanup_tmp_slots(slots: Vec<pnpr_storage::TarballSlot>) {
    for slot in slots {
        let _ = tokio::fs::remove_file(&slot.tmp_path).await;
    }
}
