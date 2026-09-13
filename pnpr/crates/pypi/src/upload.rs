use super::{BTreeMap, Upload, UploadError, multipart};
/// Read a legacy-API upload out of its parsed `multipart/form-data` parts.
pub fn parse_upload(parts: Vec<multipart::FormPart>) -> Result<Upload, UploadError> {
    let mut fields: BTreeMap<String, multipart::FormPart> = BTreeMap::new();
    for part in parts {
        fields
            .entry(part.name.clone())
            .or_insert(part);
    }
    if upload_text(&fields, ":action")?.as_deref() != Some("file_upload") {
        return Err(UploadError::NotAFileUpload);
    }
    if upload_text(&fields, "protocol_version")?.is_some_and(|version| version != "1") {
        return Err(UploadError::UnsupportedProtocolVersion);
    }
    let name = upload_text(&fields, "name")?.ok_or(UploadError::MissingField("name"))?;
    let version = upload_text(&fields, "version")?.ok_or(UploadError::MissingField("version"))?;
    let filetype = upload_text(&fields, "filetype")?.ok_or(UploadError::MissingField("filetype"))?;
    let sha256_digest = upload_text(&fields, "sha256_digest")?.filter(|digest| !digest.is_empty());
    let requires_python =
        upload_text(&fields, "requires_python")?.filter(|value| !value.is_empty());
    let content = fields
        .remove("content")
        .ok_or(UploadError::MissingField("content"))?;
    let filename = content.filename.ok_or(UploadError::MissingFilename)?;
    Ok(Upload {
        name,
        version,
        filetype,
        filename,
        content: content.data,
        sha256_digest: sha256_digest.map(|digest| digest.to_ascii_lowercase()),
        requires_python,
    })
}

fn upload_text(
    fields: &BTreeMap<String, multipart::FormPart>,
    name: &'static str,
) -> Result<Option<String>, UploadError> {
    fields
        .get(name)
        .map(|part| String::from_utf8(part.data.clone()).map_err(|_| UploadError::NotText(name)))
        .transpose()
}
