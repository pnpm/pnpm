use super::{
    CanonicalPackageName, Digest, ImageDocument, MAX_REFERRER_READ_BYTES, MAX_REFERRER_READS,
    Manifest, ManifestEntry, ReferrerMetadata, Response, Serialize, query_param, registry_error,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Referrers<'listing> {
    pub(super) schema_version: u64,
    pub(super) media_type: &'listing str,
    pub(super) manifests: Vec<ReferrerDescriptor<'listing>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReferrerDescriptor<'listing> {
    pub(super) media_type: &'listing str,
    pub(super) digest: &'listing Digest,
    pub(super) size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) artifact_type: Option<&'listing str>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub(super) annotations: &'listing std::collections::BTreeMap<String, String>,
}

impl<'listing> ReferrerDescriptor<'listing> {
    pub(super) fn new(entry: &'listing ManifestEntry, manifest: &'listing Manifest) -> Self {
        Self {
            media_type: &entry.media_type,
            digest: &entry.digest,
            size: entry.size,
            artifact_type: manifest.artifact_type(),
            annotations: manifest.annotations(),
        }
    }
}

/// What a referrers query selects.
pub(super) struct ReferrerFilter {
    pub(super) subject: Digest,
    pub(super) artifact_type: Option<String>,
    pub(super) artifact_type_digest: Option<Digest>,
}

impl ReferrerFilter {
    pub(super) fn new(subject: Digest, query: &str) -> Self {
        let artifact_type = query_param(Some(query), "artifactType");
        let artifact_type_digest = artifact_type.as_ref().map(|value| Digest::of(value.as_bytes()));
        Self { subject, artifact_type, artifact_type_digest }
    }

    /// Whether the manifest itself has to be read: either the index carries no
    /// metadata for it, or the metadata says it is a match and the descriptor
    /// needs the manifest to be built.
    pub(super) fn needs_manifest(&self, indexed: Option<&ReferrerMetadata>) -> bool {
        indexed.is_none_or(|metadata| {
            metadata.subject.as_ref() == Some(&self.subject)
                && self
                    .artifact_type_digest
                    .as_ref()
                    .is_none_or(|filter| metadata.artifact_type_digest.as_ref() == Some(filter))
        })
    }

    pub(super) fn matches(&self, manifest: &Manifest) -> bool {
        manifest.referrer_metadata().subject.as_ref() == Some(&self.subject)
            && self
                .artifact_type
                .as_deref()
                .is_none_or(|filter| manifest.artifact_type() == Some(filter))
    }
}

/// One page of a referrers scan, with the budgets that end it.
pub(super) struct ReferrerPage<'a> {
    pub(super) referrers: Vec<(&'a ManifestEntry, Manifest)>,
    /// Index entries this scan learned and should write back.
    pub(super) additions: Vec<ManifestEntry>,
    /// The last manifest this page covered, for the `Link` cursor.
    pub(super) cursor: Option<&'a Digest>,
    /// Whether manifests are left after this page.
    pub(super) more: bool,
    pub(super) max_manifest_bytes: usize,
    pub(super) inspected: usize,
    pub(super) read_bytes: u64,
    pub(super) response_bytes: usize,
}

impl<'a> ReferrerPage<'a> {
    pub(super) fn new(max_manifest_bytes: usize) -> Self {
        Self {
            referrers: Vec::new(),
            additions: Vec::new(),
            cursor: None,
            more: false,
            max_manifest_bytes,
            inspected: 0,
            read_bytes: 0,
            response_bytes: 128,
        }
    }

    /// Whether one more manifest of `size` bytes fits the read budget.
    pub(super) fn can_read(&self, size: u64) -> bool {
        self.inspected < MAX_REFERRER_READS
            && (self.inspected == 0 || self.read_bytes + size <= MAX_REFERRER_READ_BYTES)
    }

    pub(super) fn charge_read(&mut self, bytes: u64) {
        self.inspected += 1;
        self.read_bytes += bytes;
    }

    /// Take one read manifest into the page. Reports `false` when the response
    /// budget is spent and the page has to end here.
    pub(super) fn push_referrer(
        &mut self,
        entry: &'a ManifestEntry,
        manifest: Manifest,
        filter: &ReferrerFilter,
        unindexed: bool,
    ) -> Result<bool, Box<Response>> {
        if unindexed {
            let mut addition = entry.clone();
            addition.referrer = Some(manifest.referrer_metadata());
            self.additions.push(addition);
        }
        if !filter.matches(&manifest) {
            return Ok(true);
        }
        let descriptor_bytes = match serde_json::to_vec(&ReferrerDescriptor::new(entry, &manifest))
        {
            Ok(bytes) => bytes.len() + 1,
            Err(err) => return Err(Box::new(registry_error(err.into()))),
        };
        if !self.referrers.is_empty()
            && self.response_bytes + descriptor_bytes > self.max_manifest_bytes
        {
            return Ok(false);
        }
        self.response_bytes += descriptor_bytes;
        self.referrers.push((entry, manifest));
        Ok(true)
    }

    /// The `Link` header pointing at the next page, when there is one.
    pub(super) fn next_link(
        &self,
        base: &str,
        key: &CanonicalPackageName,
        filter: &ReferrerFilter,
    ) -> Option<String> {
        let cursor = self.more.then_some(self.cursor)??;
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("last", &cursor.to_string());
        if let Some(artifact_type) = filter.artifact_type.as_deref() {
            query.append_pair("artifactType", artifact_type);
        }
        Some(format!(
            r#"<{base}/{}/referrers/{}?{}>; rel="next""#,
            key.as_str(),
            filter.subject,
            query.finish(),
        ))
    }
}

/// What the scan does with one indexed manifest.
pub(super) enum ReferrerStep {
    /// Nothing here answers the query; move on.
    Skip,
    /// Read the manifest. `unindexed` says the index carried no metadata for
    /// it, so what is read has to be written back.
    Read { unindexed: bool },
    /// Re-read the document under the migration lock and judge this entry
    /// against it.
    Migrate,
    /// The page's read or response budget is spent.
    Stop,
}

pub(super) fn referrer_entry_step(
    entry: &ManifestEntry,
    indexed: Option<Option<&ReferrerMetadata>>,
    filter: &ReferrerFilter,
    page: &ReferrerPage<'_>,
    migrating: bool,
) -> ReferrerStep {
    let Some(indexed) = indexed else {
        return ReferrerStep::Skip;
    };
    if !filter.needs_manifest(indexed) {
        return ReferrerStep::Skip;
    }
    if !page.can_read(entry.size) {
        return ReferrerStep::Stop;
    }
    if indexed.is_none() && !migrating {
        return ReferrerStep::Migrate;
    }
    ReferrerStep::Read { unindexed: indexed.is_none() }
}

/// The referrer metadata the index holds for one manifest, or `None` when a
/// migrated document no longer lists it at all.
pub(super) fn indexed_referrer<'a>(
    migrated: Option<&'a ImageDocument>,
    entry: &'a ManifestEntry,
) -> Option<Option<&'a ReferrerMetadata>> {
    let Some(migrated) = migrated else {
        return Some(entry.referrer.as_ref());
    };
    Some(migrated.manifest(&entry.digest)?.referrer.as_ref())
}

/// Read the repository's image document as it stands now.
pub(super) async fn read_image_document(
    storage: &pnpr_storage::Storage,
    key: &CanonicalPackageName,
) -> Result<ImageDocument, Response> {
    match storage.read_hosted_document(key).await {
        Ok(Some(bytes)) => ImageDocument::parse(&bytes).map_err(|err| registry_error(err.into())),
        Ok(None) => Ok(ImageDocument::new(key.as_str())),
        Err(err) => Err(registry_error(err)),
    }
}
