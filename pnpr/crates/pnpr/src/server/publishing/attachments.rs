use super::{
    Action, AppState, CanonicalPackageName, Ecosystem, HashSet, HostedDocumentForUpdate,
    HostedDocumentVersion, HostedOriginalRef, Identity, Integrity, JournaledRevisionRef,
    PendingAttachment, RegistryError, RegistrySource, Storage, Value, WriteTarget,
    authorize_package, cleanup_tmp_slots, create_hex_hash, extract_attachments,
    extract_predicate_type, hosted_storage, integrity_addressed_tarball_path, json, merge_manifest,
    resolve_registry_source, resolve_write_target, stream_decode_verify_and_write,
    tar_integrity_of, tgz_integrity_key, write_sigstore_bundle,
};

/// Whether a tar registry serves `name` from the hosted registry `org`, so
/// a publish there has a tar hash worth deriving. Without `org`, any hosted
/// registry counts.
fn served_as_tar(state: &AppState, org: Option<&str>, name: &CanonicalPackageName) -> bool {
    state.inner.config.routing.tar_registries
        .iter()
        .any(|registry| match resolve_registry_source(state, registry, name.as_str()) {
            RegistrySource::Hosted(source) => org.is_none_or(|org| org == source),
            _ => false,
        })
}

/// What kind of payload an attachment carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum AttachmentKind {
    Tarball,
    Sigstore,
}

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
    pub(in super::super) kind: AttachmentKind,
}

pub(in super::super) async fn validate_publish_doc(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: CanonicalPackageName,
    mut incoming: Value,
) -> Result<(ValidatedPublish, WriteTarget), RegistryError> {
    // Route write to hosted target and enforce the registry's publish rule.
    let target = resolve_write_target(state, identity, registry, &name)?;
    authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Publish,
        Some(&target.org),
    )
    .await?;

    let storage = hosted_storage(state, Some(&target.org));
    check_publish_tombstone(state, identity, &storage, &name).await?;

    let attachments = extract_attachments(&mut incoming)?;
    record_publisher(&mut incoming, identity);

    // Validate attachment filenames against the package and parse their
    // canonical storage filename, matching version, and payload kind.
    let prepared = prepare_attachments(&name, &incoming, attachments)?;
    Ok((ValidatedPublish { name, incoming, prepared }, target))
}

async fn check_publish_tombstone(
    state: &AppState,
    identity: &Identity,
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Result<(), RegistryError> {
    let Some(tombstone) = storage.read_tombstone(name).await? else {
        return Ok(());
    };
    if storage.read_hosted_document(name).await?.is_some() {
        return Ok(());
    }
    let username = identity.username();
    let is_admin =
        username.is_some_and(|user| state.inner.config.identity.admins.contains(user));
    let is_former_maintainer = username.is_some_and(|user| {
        tombstone.maintainers
            .iter()
            .any(|maintainer| maintainer == user)
    });
    if !is_admin && !is_former_maintainer {
        return Err(RegistryError::PackageTombstoned { package: name.as_str().to_string() });
    }
    Ok(())
}

fn prepare_attachments(
    name: &CanonicalPackageName,
    incoming: &Value,
    attachments: Vec<PendingAttachment>,
) -> Result<Vec<PreparedAttachment>, RegistryError> {
    let mut prepared = Vec::with_capacity(attachments.len());
    let mut seen_canonical = HashSet::with_capacity(attachments.len());
    let mut tarball_versions = HashSet::new();

    for attachment in attachments {
        let (canonical, version, kind) = parse_attachment_kind(name, &attachment.filename)?;
        if !seen_canonical.insert(canonical.clone()) {
            return Err(RegistryError::BadRequest {
                reason: format!("duplicate attachment for {canonical}"),
            });
        }
        if kind == AttachmentKind::Tarball {
            tarball_versions.insert(version.clone());
        }
        let dist = incoming
            .get("versions")
            .and_then(|versions| versions.get(&version))
            .and_then(|manifest| manifest.get("dist"))
            .cloned()
            .unwrap_or(Value::Null);
        prepared.push(PreparedAttachment { attachment, canonical, version, dist, kind });
    }
    for att in &prepared {
        if att.kind == AttachmentKind::Sigstore && !tarball_versions.contains(&att.version) {
            return Err(RegistryError::BadRequest {
                reason: format!("sigstore attachment for {} has no tarball", att.version),
            });
        }
    }
    Ok(prepared)
}

fn parse_attachment_kind(
    name: &CanonicalPackageName,
    filename: &str,
) -> Result<(String, String, AttachmentKind), RegistryError> {
    if filename.ends_with(".tgz") {
        let (canonical, version) = name.parse_tarball_name(filename)?;
        Ok((canonical, version, AttachmentKind::Tarball))
    } else if filename.ends_with(".sigstore") || filename.ends_with(".sigstore.json") {
        let (canonical, version) = name.parse_sigstore_name(filename)?;
        Ok((canonical, version, AttachmentKind::Sigstore))
    } else {
        Err(RegistryError::InvalidTarballName {
            package: name.as_str().to_string(),
            filename: filename.to_string(),
        })
    }
}

pub(super) fn record_publisher(incoming: &mut Value, identity: &Identity) {
    if let Identity::User { username, .. } = identity {
        record_publisher_in_maintainers(incoming, username);
    }
    let Some(versions) = incoming.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for manifest in versions.values_mut().filter_map(Value::as_object_mut) {
        match identity {
            Identity::User { username, .. } => {
                manifest.insert("_npmUser".to_string(), json!({ "name": username }));
            }
            _ => {
                manifest.remove("_npmUser");
            }
        }
    }
}

fn record_publisher_in_maintainers(incoming: &mut Value, username: &str) {
    let Some(incoming_obj) = incoming.as_object_mut() else { return };
    let maintainers = incoming_obj
        .entry("maintainers")
        .or_insert_with(|| json!([]));
    let publisher = json!({ "name": username });
    let Some(list) = maintainers.as_array_mut() else {
        *maintainers = json!([publisher]);
        return;
    };
    let already_present = list
        .iter()
        .any(|entry| entry.get("name").and_then(Value::as_str) == Some(username));
    if !already_present {
        list.insert(0, publisher);
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

/// Merge the incoming packument with the on-disk / upstream state
/// and stream every tarball to a tmp slot. The caller must hold the
/// package lock for `doc.name` from before this call until after
/// [`commit_publishes`](super::commit_publishes). On error, every tmp file this call wrote is
/// removed.
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

    check_tombstoned_versions(&storage, &name, &incoming).await?;

    check_publishable_versions(&name, &incoming, hosted.as_ref(), &prepared)?;

    let document = prepare_staged_document(&name, hosted.as_ref(), &incoming, &prepared, now_iso)?;
    let original_refs = prepared
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Tarball)
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
    let derive_tars = served_as_tar(state, org, &name);
    let written_slots = write_attachment_slots(&storage, &name, prepared, derive_tars).await?;
    Ok(StagedPublish {
        name,
        ecosystem: Ecosystem::Npm,
        document,
        base_version,
        slots: written_slots,
        revision_refs: original_refs,
        org: org.map(str::to_string),
    })
}

async fn check_tombstoned_versions(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
    incoming: &Value,
) -> Result<(), RegistryError> {
    let Some(tombstone) = storage.read_tombstone(name).await? else {
        return Ok(());
    };
    let Some(incoming_versions) = incoming.get("versions").and_then(Value::as_object) else {
        return Ok(());
    };
    for version in incoming_versions.keys() {
        if tombstone.unpublished_versions.contains(version) {
            return Err(RegistryError::CannotRepublishUnpublishedVersion {
                package: name.as_str().to_string(),
                version: version.clone(),
            });
        }
    }
    Ok(())
}

fn prepare_staged_document(
    name: &CanonicalPackageName,
    hosted: Option<&Value>,
    incoming: &Value,
    prepared: &[PreparedAttachment],
    now_iso: &str,
) -> Result<Vec<u8>, RegistryError> {
    let mut merged = merge_manifest(hosted, incoming, hosted, now_iso);
    attach_sigstore_attestations(&mut merged, name, prepared);
    serde_json::to_vec_pretty(&merged).map_err(RegistryError::Json)
}

fn attach_sigstore_attestations(
    merged: &mut Value,
    name: &CanonicalPackageName,
    prepared: &[PreparedAttachment],
) {
    for attachment in prepared
        .iter()
        .filter(|att| att.kind == AttachmentKind::Sigstore)
    {
        if let Some(dist) = merged
            .get_mut("versions")
            .and_then(|v| v.get_mut(&attachment.version))
            .and_then(|m| m.get_mut("dist"))
            .and_then(Value::as_object_mut)
        {
            let predicate_type = extract_predicate_type(&attachment.attachment.data);
            dist.insert(
                "attestations".to_string(),
                json!({
                    "url": format!("/-/npm/v1/attestations/{}@{}", name.as_str(), attachment.version),
                    "provenance": { "predicateType": predicate_type },
                }),
            );
        }
    }
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
    derive_tars: bool,
) -> Result<Vec<pnpr_storage::BlobSlot>, RegistryError> {
    let mut written_slots = Vec::with_capacity(prepared.len());
    for attachment in prepared {
        match write_attachment_slot(storage, name, attachment, derive_tars).await {
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
    derive_tars: bool,
) -> Result<pnpr_storage::BlobSlot, RegistryError> {
    let slot = storage.reserve_hosted_blob(name, &prepared.canonical).await?;
    let tmp_path = slot.tmp_path.clone();
    let tgz_integrity = match prepared.kind {
        AttachmentKind::Tarball if derive_tars => {
            prepared.dist.as_object().and_then(tgz_integrity_key)
        }
        AttachmentKind::Tarball => None,
        AttachmentKind::Sigstore => None,
    };
    let result = tokio::task::spawn_blocking(move || {
        decode_and_write_attachment(
            &prepared.attachment,
            prepared.kind,
            &prepared.dist,
            &tmp_path,
        )?;
        // A tar registry describes the version by the tar this `.tgz` decodes to.
        let tar_integrity = tgz_integrity
            .zip(std::fs::File::open(&tmp_path).ok())
            .and_then(|(key, tgz)| Some((key, tar_integrity_of(std::io::BufReader::new(tgz))?)));
        Ok::<_, RegistryError>(tar_integrity)
    })
    .await;
    match result {
        Ok(Ok(tar_integrity)) => {
            // The first tar response derives a hash publish failed to record.
            if let Some((key, tar_integrity)) = tar_integrity
                && let Err(err) =
                    storage.record_tar_integrity(name, &key, &tar_integrity.to_string()).await
            {
                tracing::warn!(?err, package = %name.as_str(), "recording a tar integrity failed");
            }
            Ok(slot)
        }
        Ok(Err(err)) => Err(err),
        Err(join_err) => {
            let _ = tokio::fs::remove_file(&slot.tmp_path).await;
            Err(RegistryError::Io(std::io::Error::other(join_err.to_string())))
        }
    }
}

fn decode_and_write_attachment(
    attachment: &PendingAttachment,
    kind: AttachmentKind,
    dist: &Value,
    tmp_path: &std::path::Path,
) -> Result<(), RegistryError> {
    let dist_ref = (!dist.is_null()).then_some(dist);
    match kind {
        AttachmentKind::Tarball => {
            stream_decode_verify_and_write(
                &attachment.filename,
                &attachment.data,
                attachment.declared_length,
                dist_ref,
                tmp_path,
            )?;
        }
        AttachmentKind::Sigstore => {
            write_sigstore_bundle(
                &attachment.filename,
                &attachment.data,
                attachment.declared_length,
                tmp_path,
            )?;
        }
    }
    Ok(())
}

/// Validate each incoming version against the locally hosted packument. A
/// hosted packument is served as-is, so anything not in it is genuinely new
/// Validate versions against existing hosted versions. Published content is
/// immutable, so re-publishes with modified tarballs or integrity are rejected.
/// New versions must provide a tarball.
pub(super) fn check_publishable_versions(
    name: &CanonicalPackageName,
    incoming: &Value,
    hosted: Option<&Value>,
    prepared: &[PreparedAttachment],
) -> Result<(), RegistryError> {
    let attachment_versions: HashSet<&str> = prepared
        .iter()
        .filter(|attachment| attachment.kind == AttachmentKind::Tarball)
        .map(|attachment| attachment.version.as_str())
        .collect();
    let hosted_versions = hosted
        .and_then(|h| h.get("versions"))
        .and_then(Value::as_object);
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
    let integrity: Integrity = attachment.dist
        .get("integrity")?
        .as_str()?
        .parse()
        .ok()?;
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
