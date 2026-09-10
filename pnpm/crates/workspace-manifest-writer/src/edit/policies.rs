use super::{
    Inline, Manifest, flow, insert_top_level_block, locate, locate_sequence, mapping_keys,
    reconcile_sequence_items, remove_mapping_entries, remove_top_level_block, render,
    render_top_level_sequence, replace_top_level_block, top_level_span, upsert_sequence_entry,
};

/// Set the ignore list to `ghsas` (the complete desired list) in whichever
/// spelling the manifest uses — the canonical `audit.ignore` wins over the
/// deprecated `auditConfig.ignoreGhsas`, matching the reader's precedence,
/// so a stale canonical list can't shadow the update on the next read. When
/// both spellings are present, the shadowed deprecated list is removed as
/// part of the write. `auditConfig.ignoreGhsas` is created when neither is
/// present. An empty `ghsas` removes the list, dropping its block when
/// nothing else remains in it. `pnpm audit --ignore` and `audit.ignorePrune`
/// call this with the complete desired list. Returns whether anything
/// changed.
pub(crate) fn set_audit_ignore_ghsas(
    manifest: &mut Manifest,
    ghsas: &[String],
) -> Result<bool, Box<yamlpatch::Error>> {
    if manifest.audit_ignore.is_some() {
        return Ok(set_audit_ignore(manifest, ghsas));
    }

    const BLOCK: &str = "auditConfig";
    if ghsas.is_empty() {
        return Ok(remove_audit_config_ghsas(manifest, BLOCK));
    }
    if manifest.audit_ignore_ghsas.as_deref().unwrap_or_default() == ghsas {
        return Ok(false);
    }

    let text = manifest.text();
    if locate(text, &[BLOCK]).is_some() {
        let new_text = upsert_sequence_entry(text, BLOCK, "ignoreGhsas", ghsas);
        manifest.set_text(new_text);
    } else {
        let block = render_audit_config_block(ghsas);
        let new_text = insert_top_level_block(manifest, BLOCK, &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[BLOCK.to_string()]);
    }
    manifest.audit_ignore_ghsas = Some(ghsas.to_vec());
    Ok(true)
}

/// A manifest already using `audit.ignore` keeps using it, and drops the
/// legacy `auditConfig.ignoreGhsas` it also carries.
fn set_audit_ignore(manifest: &mut Manifest, ghsas: &[String]) -> bool {
    let mut changed = if ghsas.is_empty() {
        remove_block_list_key(manifest, "audit", "ignore");
        manifest.audit_ignore = None;
        true
    } else if manifest.audit_ignore.as_deref() == Some(ghsas) {
        false
    } else {
        let new_text = upsert_sequence_entry(manifest.text(), "audit", "ignore", ghsas);
        manifest.set_text(new_text);
        manifest.audit_ignore = Some(ghsas.to_vec());
        true
    };
    if manifest.audit_ignore_ghsas.is_some() {
        remove_block_list_key(manifest, "auditConfig", "ignoreGhsas");
        manifest.audit_ignore_ghsas = None;
        changed = true;
    }
    changed
}

fn remove_audit_config_ghsas(manifest: &mut Manifest, block: &str) -> bool {
    let text = manifest.text();
    // Nothing to remove if `ignoreGhsas` isn't present — and crucially,
    // don't touch sibling `auditConfig` keys.
    if locate(text, &[block]).is_none()
        || !mapping_keys(text, &[block]).iter().any(|key| key == "ignoreGhsas")
    {
        return false;
    }
    remove_block_list_key(manifest, block, "ignoreGhsas");
    manifest.audit_ignore_ghsas = None;
    true
}

/// Remove `block.key` from the document — the whole `block:` when the key is
/// its only entry, so no empty mapping is left behind. Sibling keys of
/// `block` are never touched. A missing block or key is a no-op.
fn remove_block_list_key(manifest: &mut Manifest, block: &str, key: &str) {
    let text = manifest.text();
    if locate(text, &[block]).is_none() {
        return;
    }
    let keys = mapping_keys(text, &[block]);
    if !keys.iter().any(|k| k == key) {
        return;
    }
    if keys.iter().all(|k| k == key) {
        let new_text = remove_top_level_block(text, block);
        manifest.set_text(new_text);
        manifest.top_level_keys.retain(|k| k != block);
    } else {
        let new_text = remove_mapping_entries(text, &[block], &[key.to_string()]);
        manifest.set_text(new_text);
    }
}

/// A top-level exclude list of package/version specs.
#[derive(Clone, Copy)]
struct ExcludeList {
    /// The `pnpm-workspace.yaml` key holding the list.
    key: &'static str,
    /// The decoded copy [`Manifest`] keeps, read for no-op detection and
    /// kept in step with every text edit.
    decoded: fn(&mut Manifest) -> &mut Option<Vec<String>>,
}

const MINIMUM_RELEASE_AGE_EXCLUDE: ExcludeList = ExcludeList {
    key: "minimumReleaseAgeExclude",
    decoded: |manifest| &mut manifest.minimum_release_age_exclude,
};

const TRUST_POLICY_EXCLUDE: ExcludeList = ExcludeList {
    key: "trustPolicyExclude",
    decoded: |manifest| &mut manifest.trust_policy_exclude,
};

/// Set the top-level `minimumReleaseAgeExclude:` block to `items` (the
/// complete desired list), creating or replacing it, and removing it when
/// `items` is empty. The caller is responsible for merging with the existing
/// entries (via `pnpm_config::version_policy::merge_package_version_specs`)
/// before calling. Returns whether anything changed.
pub(crate) fn set_minimum_release_age_excludes(manifest: &mut Manifest, items: &[String]) -> bool {
    set_exclude_list(manifest, MINIMUM_RELEASE_AGE_EXCLUDE, items)
}

/// Set `list`'s top-level block to `items` (the complete desired list),
/// creating or replacing it, and removing it when `items` is empty. A
/// block-style list whose entries are already on disk is reconciled entry by
/// entry — an entry whose value survives keeps its lines, comments included,
/// and only the changed entries are re-rendered. Returns whether anything
/// changed.
fn set_exclude_list(manifest: &mut Manifest, list: ExcludeList, items: &[String]) -> bool {
    let ExcludeList { key: block, decoded } = list;

    if items.is_empty() {
        return remove_exclude_list(manifest, list);
    }

    if decoded(manifest).as_deref().unwrap_or_default() == items {
        return false;
    }
    // `text` borrows the manifest for the rest of the write, so the
    // reconciliation reads the current entries from a copy.
    let current: Vec<String> = decoded(manifest).as_deref().unwrap_or_default().to_vec();

    let text = manifest.text();
    match locate_sequence(text, &[block]) {
        Inline::Flow(collection) => {
            let rendered: Vec<String> =
                items.iter().map(|item| render::render_value(item)).collect();
            manifest.set_text(flow::set_items(text, &collection, &rendered));
            *decoded(manifest) = Some(items.to_vec());
            return true;
        }
        // Rendering the whole block afresh would drop the comments an
        // inline value this writer cannot edit may hold, so leave it be;
        // the public writer refuses such a manifest outright.
        Inline::Unsupported => return false,
        Inline::Block => {}
    }

    if let Some(new_text) = reconcile_sequence_items(text, block, &current, items) {
        manifest.set_text(new_text);
        *decoded(manifest) = Some(items.to_vec());
        return true;
    }

    let rendered = render_top_level_sequence(block, items);
    if let Some(span) = top_level_span(text, block) {
        manifest.set_text(replace_top_level_block(text, &span, &rendered));
    } else {
        let new_text = insert_top_level_block(manifest, block, &rendered);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[block.to_string()]);
    }
    *decoded(manifest) = Some(items.to_vec());
    true
}

fn remove_exclude_list(manifest: &mut Manifest, list: ExcludeList) -> bool {
    let ExcludeList { key: block, decoded } = list;
    let has_block = manifest.top_level_keys.iter().any(|key| key == block);
    if !has_block {
        return false;
    }
    manifest.set_text(remove_top_level_block(manifest.text(), block));
    *decoded(manifest) = None;
    manifest.top_level_keys.retain(|key| key != block);
    true
}

/// The `minimumReleaseAgeExcludePrune` pass over `minimumReleaseAgeExclude:`.
/// Returns whether anything changed.
pub(crate) fn prune_minimum_release_age_excludes(
    manifest: &mut Manifest,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    prune_exclude_list(manifest, MINIMUM_RELEASE_AGE_EXCLUDE, resolved)
}

/// The `trustPolicyExcludePrune` pass over `trustPolicyExclude:`. Returns
/// whether anything changed.
pub(crate) fn prune_trust_policy_excludes(
    manifest: &mut Manifest,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    prune_exclude_list(manifest, TRUST_POLICY_EXCLUDE, resolved)
}

/// Prune `list`'s entries against the versions the freshly resolved lockfile
/// records. The per-entry decision lives in
/// [`pnpm_config::version_policy::drop_unresolved_package_version_specs`]; the
/// write goes through [`set_exclude_list`]'s entry-by-entry reconciliation, so
/// the comments of the surviving entries stay, a pruned-to-empty list drops
/// the block, and an unchanged list is a no-op. A list that is absent or
/// already empty is left verbatim — it has nothing to prune, and dropping the
/// block would diverge from pnpm. Returns whether anything changed.
fn prune_exclude_list(
    manifest: &mut Manifest,
    list: ExcludeList,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    let current = (list.decoded)(manifest).as_deref().unwrap_or_default();
    if current.is_empty() {
        return false;
    }
    let pruned =
        pnpm_config::version_policy::drop_unresolved_package_version_specs(current, resolved);
    set_exclude_list(manifest, list, &pruned)
}

/// Render a brand-new `auditConfig:` block holding `ignoreGhsas`. GHSA IDs are
/// plain scalars, but route through [`render::render_value`] for safety.
fn render_audit_config_block(ghsas: &[String]) -> String {
    let mut block = String::from("auditConfig:\n  ignoreGhsas:\n");
    for ghsa in ghsas {
        block.push_str("    - ");
        block.push_str(&render::render_value(ghsa));
        block.push('\n');
    }
    block
}
