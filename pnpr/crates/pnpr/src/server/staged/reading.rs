use super::{
    AppState, BASE64, Body, Engine, Identity, IntoResponse, MAX_PER_PAGE, RegistryError, Response,
    StagedListQuery, StagedRecord, StatusCode, Value, authorize_staged, extract_attachments,
    header, json, json_response, load_authorized_record, not_found, read_staged_record,
};
/// `GET /-/stage?page=&perPage=&package=` — the staged records visible to
/// the caller through this registry address, sorted oldest-first by staging
/// time (then id) so pagination stays stable as new records arrive.
pub(super) async fn serve_staged_list(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    query: &StagedListQuery,
) -> Response {
    let per_page = query.per_page.clamp(1, MAX_PER_PAGE);
    let ids = match state.inner.storage.list_staged_ids().await {
        Ok(ids) => ids,
        Err(err) => return err.into_response(),
    };
    let mut records: Vec<StagedRecord> = Vec::new();
    for stage_id in ids {
        let Ok(Some(stored)) = read_staged_record(state, &stage_id).await else {
            continue;
        };
        let record = stored.record;
        if record.registry.as_deref() != registry {
            continue;
        }
        if let Some(package) = &query.package
            && &record.package_name != package
        {
            continue;
        }
        // The listing shows only what the caller could publish (and thus
        // approve); records outside their rights are simply not theirs to see.
        if authorize_staged(state, identity, &record).await.is_err() {
            continue;
        }
        records.push(record);
    }
    records.sort_by(staged_record_order);

    let total = records.len();
    let items: Vec<Value> = records
        .iter()
        .skip(query.page.saturating_mul(per_page))
        .take(per_page)
        .map(StagedRecord::metadata)
        .collect();
    json_response(
        StatusCode::OK,
        &json!({ "items": items, "page": query.page, "perPage": per_page, "total": total }),
    )
}

/// `GET /-/stage/:id` — one staged record's metadata.
pub(super) async fn serve_staged_view(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    let stored = match load_authorized_record(state, identity, registry, stage_id).await {
        Ok(stored) => stored,
        Err(err) => return err.into_response(),
    };
    json_response(StatusCode::OK, &stored.record.metadata())
}

/// `GET /-/stage/:id/tarball` — the held tarball's bytes, decoded from the
/// stored publish document's attachment.
pub(super) async fn serve_staged_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> Response {
    if let Err(response) = load_authorized_record(state, identity, registry, stage_id).await {
        return response.into_response();
    }
    let body = match state.inner.storage.read_staged_body(stage_id).await {
        Ok(Some(body)) => body,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let mut incoming: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => return RegistryError::Json(err).into_response(),
    };
    let attachments = match extract_attachments(&mut incoming) {
        Ok(attachments) => attachments,
        Err(err) => return err.into_response(),
    };
    let Some(attachment) = attachments.into_iter().next() else {
        return not_found();
    };
    let bytes = match BASE64.decode(attachment.data.as_bytes()) {
        Ok(bytes) => bytes,
        Err(err) => {
            return RegistryError::InvalidAttachment {
                filename: attachment.filename,
                reason: format!("invalid base64 data: {err}"),
            }
            .into_response();
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

fn staged_record_order(left: &StagedRecord, right: &StagedRecord) -> std::cmp::Ordering {
    left.created_at
        .cmp(&right.created_at)
        .then_with(|| left.id.cmp(&right.id))
}
