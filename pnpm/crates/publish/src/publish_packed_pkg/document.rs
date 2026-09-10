use super::{
    Access, Map, NormalizedRegistryUrl, PublishPackedPkgError, Value,
    is_valid_old_npm_package_name, join_registry,
};

/// The tarball digests written into the document's `dist`, already computed by
/// [`create_publish_summary`](crate::publish_summary::create_publish_summary) so the tarball is not hashed twice.
pub(crate) struct DistHashes<'a> {
    /// SRI SHA-512 (`sha512-...`).
    pub(crate) integrity: &'a str,
    /// Lowercase hex SHA-1.
    pub(crate) shasum: &'a str,
}

/// Build the npm publish document — the JSON body sent as the whole
/// `PUT /:pkg` request.
pub(crate) fn build_publish_document(
    manifest: &Value,
    tarball_data: &[u8],
    registry: &NormalizedRegistryUrl,
    access: Option<Access>,
    tag: &str,
    dist_hashes: &DistHashes<'_>,
) -> Result<Value, PublishPackedPkgError> {
    if manifest.get("private").and_then(Value::as_bool) == Some(true) {
        return Err(PublishPackedPkgError::Private);
    }
    let name = manifest_string(manifest, "name");
    // Validate the name before it flows into the tarball URI and (in the caller)
    // the authenticated PUT URL, mirroring libnpmpublish's `npa.resolve(name, ..)`
    // gate. Without it a crafted tarball's `package.json` name could parse as an
    // absolute URL under `Url::join` (special-scheme URLs also treat `\` as `/`)
    // and redirect the publish request — carrying the `Authorization` / `npm-otp`
    // headers — to an attacker-controlled host.
    if !is_valid_old_npm_package_name(&name) {
        return Err(PublishPackedPkgError::InvalidPackageName { name });
    }
    let version = clean_version(&manifest_string(manifest, "version"))?;

    if !name.starts_with('@') && access == Some(Access::Restricted) {
        return Err(PublishPackedPkgError::UnscopedRestricted { name });
    }

    let tarball_name = format!("{name}-{version}.tgz");
    let tarball_url = join_registry(registry, &format!("{name}/-/{tarball_name}"))?
        .replacen("https://", "http://", 1);
    let versions =
        versions_object(manifest, &name, &version, dist_object(dist_hashes, tarball_url));

    // A manifest-level `tag` wins over the default.
    let tag = manifest.get("tag").and_then(Value::as_str).unwrap_or(tag);
    let mut dist_tags = Map::new();
    dist_tags.insert(tag.to_owned(), Value::String(version));

    let mut attachments = Map::new();
    attachments.insert(tarball_name, attachment_object(tarball_data));

    let mut root = Map::new();
    root.insert("_id".to_owned(), Value::String(name.clone()));
    root.insert("name".to_owned(), Value::String(name));
    if let Some(description) = manifest.get("description").filter(|value| value.is_string()) {
        root.insert("description".to_owned(), description.clone());
    }
    root.insert("dist-tags".to_owned(), Value::Object(dist_tags));
    root.insert("versions".to_owned(), Value::Object(versions));
    root.insert(
        "access".to_owned(),
        access.map_or(Value::Null, |access| Value::String(access.to_string())),
    );
    root.insert("_attachments".to_owned(), Value::Object(attachments));
    Ok(Value::Object(root))
}

fn dist_object(dist_hashes: &DistHashes<'_>, tarball_url: String) -> Map<String, Value> {
    let mut dist = Map::new();
    dist.insert("integrity".to_owned(), Value::String(dist_hashes.integrity.to_owned()));
    dist.insert("shasum".to_owned(), Value::String(dist_hashes.shasum.to_owned()));
    dist.insert("tarball".to_owned(), Value::String(tarball_url));
    dist
}

/// The `versions` map holding the one published version: the manifest with
/// its `_id`, `version` and `dist` set.
fn versions_object(
    manifest: &Value,
    name: &str,
    version: &str,
    dist: Map<String, Value>,
) -> Map<String, Value> {
    let mut version_manifest = manifest.as_object().cloned().unwrap_or_default();
    version_manifest.insert("_id".to_owned(), Value::String(format!("{name}@{version}")));
    version_manifest.insert("version".to_owned(), Value::String(version.to_owned()));
    version_manifest.insert("dist".to_owned(), Value::Object(dist));

    let mut versions = Map::new();
    versions.insert(version.to_owned(), Value::Object(version_manifest));
    versions
}

fn attachment_object(tarball_data: &[u8]) -> Value {
    serde_json::json!({
        "content_type": "application/octet-stream",
        "data": base64_standard(tarball_data),
        "length": tarball_data.len(),
    })
}

/// Clean a version string to `major.minor.patch` plus any prerelease,
/// dropping build metadata.
pub(super) fn clean_version(version: &str) -> Result<String, PublishPackedPkgError> {
    let trimmed = version.trim().trim_start_matches(['=', 'v']);
    let mut parsed = trimmed
        .parse::<node_semver::Version>()
        .map_err(|_| PublishPackedPkgError::BadSemver { version: version.to_owned() })?;
    // The published version is `major.minor.patch` plus any prerelease but
    // never build metadata. node_semver's `Display` appends `+build`, so drop
    // it to keep the published version identical to what pnpm registers (e.g.
    // `1.2.3+build` -> `1.2.3`).
    parsed.build.clear();
    Ok(parsed.to_string())
}

fn base64_standard(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

pub(super) fn manifest_string(manifest: &Value, key: &str) -> String {
    manifest.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}
