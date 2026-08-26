use indexmap::IndexMap;
use pnpm_crypto_hash::{create_hex_hash, integrity_addressed_tarball_path};
use pnpm_lockfile::TarballRevision;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ssri::Integrity;

#[derive(Serialize, Deserialize)]
pub(crate) struct HostedOriginalRef {
    pub(crate) package: String,
    pub(crate) version: String,
}

pub(crate) struct HostedOriginalReference {
    pub(crate) digest: String,
    pub(crate) ref_id: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn hosted_original_reference(
    package: &str,
    version: &str,
    integrity: &Integrity,
) -> Option<HostedOriginalReference> {
    let path = integrity_addressed_tarball_path(integrity)?;
    let digest = path.strip_prefix("-/tarballs/sha512/")?.to_string();
    let record = HostedOriginalRef { package: package.to_string(), version: version.to_string() };
    let bytes = serde_json::to_vec(&record).expect("hosted original reference serializes");
    let ref_id = create_hex_hash(&format!("{}\0{}", record.package, record.version));
    Some(HostedOriginalReference { digest, ref_id, bytes })
}

#[derive(Deserialize)]
pub(crate) struct HostedRevisionPackument {
    #[serde(default)]
    pub(crate) versions: IndexMap<String, HostedRevisionManifest>,
}

#[derive(Deserialize)]
pub(crate) struct HostedRevisionManifest {
    #[serde(default)]
    pub(crate) dist: Option<HostedRevisionDist>,
}

#[derive(Deserialize)]
pub(crate) struct HostedRevisionDist {
    #[serde(default)]
    pub(crate) integrity: Option<String>,
    #[serde(default)]
    pub(crate) revision: RevisionField,
    #[serde(default)]
    pub(crate) revisions: Vec<HostedRevisionRecord>,
}

#[derive(Deserialize)]
pub(crate) struct HostedRevisionRecord {
    #[serde(default)]
    pub(crate) revision: Value,
    #[serde(default)]
    pub(crate) integrity: Option<String>,
}

#[derive(Default)]
pub(crate) enum RevisionField {
    #[default]
    Missing,
    Present(Value),
}

impl<'de> Deserialize<'de> for RevisionField {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        <Value as Deserialize>::deserialize(deserializer).map(Self::Present)
    }
}

pub(crate) fn original_integrity(dist: &HostedRevisionDist) -> Option<Integrity> {
    let RevisionField::Present(revision) = &dist.revision else {
        return dist.integrity.as_deref()?.parse().ok();
    };
    let selected_revision =
        revision.as_u64().and_then(|revision| TarballRevision::try_from(revision).ok())?.get();
    let selected: Vec<_> = dist
        .revisions
        .iter()
        .filter(|record| record.revision.as_u64() == Some(selected_revision))
        .collect();
    if selected.len() != 1 || selected[0].integrity.as_deref() != dist.integrity.as_deref() {
        return None;
    }
    let originals: Vec<_> =
        dist.revisions.iter().filter(|record| record.revision.as_u64() == Some(0)).collect();
    if originals.len() != 1 {
        return None;
    }
    originals[0].integrity.as_deref()?.parse().ok()
}
