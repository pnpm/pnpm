use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentForUpdate,
    HostedDocumentVersion, HostedOriginalRef, Identity, Integrity, JournaledRevisionRef,
    PendingAttachment, RegistryError, RegistrySource, Storage, Value, WriteTarget, authorize,
    cleanup_tmp_slots, create_hex_hash, extract_attachments, hosted_storage,
    integrity_addressed_tarball_path, json, merge_manifest, resolve_write_target,
    stream_decode_verify_and_write,
};

/// A publish document that passed every check that can run before
/// taking the package lock: the caller may publish the package, and
/// each attachment maps to a canonical disk filename and a
/// `versions[v].dist` block.
pub(in super::super) struct ValidatedPublish {
    pub(in super::super) name: CanonicalPackageName,
    /// The publish body with `_attachments` stripped.
    pub(in super::super) incoming: Value,
    /// One entry per attachment.
    pub(in super::super) prepared: Vec<PreparedAttachment>,
}

/// One publish attachment resolved to its canonical on-disk filename and its
/// `versions[version].dist` block.
pub(in super::super) struct PreparedAttachment {
    pub(super) attachment: PendingAttachment,
    /// Canonical on-disk filename.
    pub(super) canonical: String,
    /// The version this attachment publishes, parsed from its filename.
    /// Lets the re-publish guard tell a content publish from a metadata-only
    /// update (which carries no attachments).
    pub(in super::super) version: String,
    /// The matching `dist` block, or `Value::Null` when absent.
    pub(in super::super) dist: Value,
}

pub(in super::super) async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: CanonicalPackageName,
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
    record_publisher(&mut incoming, identity);

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

pub(super) fn record_publisher(incoming: &mut Value, identity: &Identity) {
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for manifest in versions.values_mut().filter_map(Value::as_object_mut) {
        match identity {
            Identity::User { username } => {
                manifest.insert("_npmUser".to_string(), json!({ "name": username }));
            }
            Identity::Anonymous => {
                manifest.remove("_npmUser");
            }
        }
    }
}

/// A publish whose document is computed and whose blobs are fully written to
/// tmp slots — everything verified, nothing visible to readers yet.
/// [`commit_publishes`](super::commit_publishes) makes it visible. Every surface stages into this, so
/// one commit can carry packages of more than one ecosystem.
pub(in super::super) struct StagedPublish {
    pub(in super::super) name: CanonicalPackageName,
    pub(in super::super) ecosystem: Ecosystem,
    /// The document to record, merged with what the store held when it was
    /// read: a packument, a crate document, a project document.
    pub(in super::super) document: Vec<u8>,
    pub(in super::super) base_version: Option<HostedDocumentVersion>,
    pub(in super::super) slots: Vec<pnpr_storage::BlobSlot>,
    pub(in super::super) revision_refs: Vec<JournaledRevisionRef>,
    /// Hosted-org storage namespace this publish targets, or `None` for the
    /// flat (path-less) hosted store. Threaded into the commit and journal so
    /// the write — and any crash recovery — lands in the right org.
    pub(in super::super) org: Option<String>,
}

pub(in super::super) async fn stage_publish(
    state: &AppState,
    doc: ValidatedPublish,
    now_iso: &str,
    org: Option<&str>,
) -> Result<StagedPublish, RegistryError> {
    let ValidatedPublish { name, incoming, prepared } = doc;
    let storage = hosted_storage(state, org);

    let (hosted, base_version) =
        parse_hosted_packument(storage.read_hosted_document_for_update(&name).await?)?;

    check_publishable_versions(&name, &incoming, hosted.as_ref(), &prepared)?;

    // A hosted registry has no upstream, so a publish seeds the merge only from
    // the org's own hosted packument; a brand-new package starts from `None`.
    let merged = merge_manifest(hosted.as_ref(), &incoming, hosted.as_ref(), now_iso);
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
    let written_slots = write_attachment_slots(&storage, &name, prepared).await?;
    Ok(StagedPublish {
        name,
        ecosystem: Ecosystem::Npm,
        document: merged_bytes,
        base_version,
        slots: written_slots,
        revision_refs: original_refs,
        org: org.map(str::to_string),
    })
}

/// The stored packument, parsed, with the version its update must be based
/// on. `None` for a package the org has not published yet.
pub(super) fn parse_hosted_packument(
    hosted: Option<HostedDocumentForUpdate>,
) -> Result<(Option<Value>, Option<HostedDocumentVersion>), RegistryError> {
    let Some(packument) = hosted else {
        return Ok((None, None));
    };
    let value = serde_json::from_slice(&packument.bytes).map_err(RegistryError::Json)?;
    Ok((Some(value), Some(packument.version)))
}

/// Write every attachment into its reserved blob slot. A failure removes
/// the slots written so far, so a bad upload leaves no on-disk artifact.
pub(super) async fn write_attachment_slots(
    storage: &Storage,
    name: &CanonicalPackageName,
    prepared: Vec<PreparedAttachment>,
) -> Result<Vec<pnpr_storage::BlobSlot>, RegistryError> {
    let mut written_slots = Vec::with_capacity(prepared.len());
    for attachment in prepared {
        match write_attachment_slot(storage, name, attachment).await {
            Ok(slot) => written_slots.push(slot),
            Err(err) => {
                cleanup_tmp_slots(written_slots).await;
                return Err(err);
            }
        }
    }
    Ok(written_slots)
}

/// Stream-decode, verify and write one tarball into a reserved slot. A
/// mismatch, or a missing integrity field, fails with a 400.
pub(super) async fn write_attachment_slot(
    storage: &Storage,
    name: &CanonicalPackageName,
    prepared: PreparedAttachment,
) -> Result<pnpr_storage::BlobSlot, RegistryError> {
    let PreparedAttachment { attachment, canonical, version: _, dist } = prepared;
    let slot = storage.reserve_hosted_blob(name, &canonical).await?;
    let PendingAttachment { filename, data, declared_length } = attachment;
    let tmp_path = slot.tmp_path.clone();
    let dist_for_task = (!dist.is_null()).then_some(dist);
    let result = tokio::task::spawn_blocking(move || {
        stream_decode_verify_and_write(
            &filename,
            &data,
            declared_length,
            dist_for_task.as_ref(),
            &tmp_path,
        )
    })
    .await;
    match result {
        Ok(Ok(_)) => Ok(slot),
        Ok(Err(err)) => Err(err),
        Err(join_err) => {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            Err(RegistryError::Io(std::io::Error::other(join_err.to_string())))
        }
    }
}

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`](super::commit_publishes). On error, every tmp file this call wrote is
/// removed.
/// Validate each incoming version against the locally hosted packument. A
/// hosted packument is served as-is, so anything not in it is genuinely new
/// here, even if it exists upstream.
///
/// * Already hosted — published content is immutable, so a *content*
///   re-publish is refused with 409 (as npm/verdaccio do): one that carries a
///   new tarball (an attachment) or changes `dist.integrity` (the content
///   anchor; the `tarball` URL is rewritten on read, so it is not compared).
///   A clash that does neither is a metadata-only update (`pnpm deprecate`),
///   which is allowed — `merge_versions` keeps the hosted `dist`.
/// * New — it must ship a tarball. A version entry with no attachment would be
///   advertised with no hosted tarball (installs 404) and would block a later
///   real publish of it (409), so it is refused with 400.
pub(super) fn check_publishable_versions(
    name: &CanonicalPackageName,
    incoming: &Value,
    hosted: Option<&Value>,
    prepared: &[PreparedAttachment],
) -> Result<(), RegistryError> {
    let attachment_versions: HashSet<&str> =
        prepared.iter().map(|attachment| attachment.version.as_str()).collect();
    let hosted_versions = hosted.and_then(|h| h.get("versions")).and_then(Value::as_object);
    let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) else {
        return Ok(());
    };
    for (version, incoming_manifest) in incoming_versions {
        let has_attachment = attachment_versions.contains(version.as_str());
        let Some(hosted_manifest) = hosted_versions.and_then(|hosted| hosted.get(version)) else {
            if has_attachment {
                continue;
            }
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "cannot publish version {version} of {:?} without a tarball",
                    name.as_str(),
                ),
            });
        };
        if has_attachment || integrity_changed(incoming_manifest, hosted_manifest) {
            return Err(RegistryError::VersionAlreadyPublished {
                package: name.as_str().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(())
}

/// Whether the incoming manifest declares an integrity other than the one
/// the hosted manifest carries.
pub(super) fn integrity_changed(incoming_manifest: &Value, hosted_manifest: &Value) -> bool {
    let incoming_integrity = incoming_manifest.pointer("/dist/integrity").and_then(Value::as_str);
    let hosted_integrity = hosted_manifest.pointer("/dist/integrity").and_then(Value::as_str);
    incoming_integrity.is_some_and(|integrity| Some(integrity) != hosted_integrity)
}

pub(super) fn staged_hosted_original_ref(
    package: &CanonicalPackageName,
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
