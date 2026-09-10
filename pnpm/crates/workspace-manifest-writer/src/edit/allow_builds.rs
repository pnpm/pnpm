use super::{
    AllowBuildValue, HashSet, IndexMap, Inline, Manifest, flow, insert_top_level_block, locate,
    locate_mapping, mapping_keys, remove_top_level_block, render, replace_bool_value_at,
    write_rendered_entry_at,
};

/// Upsert one `name → bool` entry into the top-level `allowBuilds:` block,
/// creating the block if absent. Returns whether anything changed. `pnpm
/// approve-builds` calls this with each approved package set to `true` and
/// each denied/unselected package set to `false`.
pub(crate) fn add_allow_build(manifest: &mut Manifest, name: &str, value: bool) -> bool {
    const BLOCK: &str = "allowBuilds";
    let changed = if locate(manifest.text(), &[BLOCK]).is_some() {
        let Some(changed) = write_allow_build(manifest, BLOCK, name, value) else {
            return false;
        };
        changed
    } else {
        let block = format!("{BLOCK}:\n  {}: {}\n", render::render_value(name), render_bool(value));
        let new_text = insert_top_level_block(manifest, BLOCK, &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[BLOCK.to_string()]);
        true
    };
    // Keep the decoded view in sync so later upserts in the same write see
    // this entry (for both no-op detection and block-presence checks).
    manifest
        .allow_builds
        .get_or_insert_with(IndexMap::new)
        .insert(name.to_string(), AllowBuildValue::Bool(value));
    changed
}

/// Write one entry into an existing `allowBuilds:` block. `None` when the
/// entry already holds this value: rewriting the file would bump its mtime
/// for nothing.
fn write_allow_build(
    manifest: &mut Manifest,
    block: &str,
    name: &str,
    value: bool,
) -> Option<bool> {
    let text = manifest.text();
    if !mapping_keys(text, &[block]).iter().any(|key| key == name) {
        let new_text = write_rendered_entry_at(text, &[block], name, render_bool(value));
        manifest.set_text(new_text);
        return Some(true);
    }
    if manifest.allow_builds.as_ref().and_then(|builds| builds.get(name))
        == Some(&AllowBuildValue::Bool(value))
    {
        return None;
    }
    let new_text = if let Inline::Flow(collection) = locate_mapping(text, &[block]) {
        flow::upsert(text, &collection, name, render_bool(value))
    } else {
        replace_bool_value_at(text, &[block], name, value)
    };
    manifest.set_text(new_text);
    Some(true)
}

/// Add `name: <placeholder>` to the `allowBuilds:` block, creating the
/// block when absent. An entry that already exists is left alone whatever
/// its value, so a recorded decision — or a placeholder a previous install
/// wrote — survives. Returns whether the document changed.
pub(crate) fn add_undecided_allow_build(
    manifest: &mut Manifest,
    name: &str,
    placeholder: &str,
) -> bool {
    const BLOCK: &str = "allowBuilds";
    let text = manifest.text();
    if locate(text, &[BLOCK]).is_some() {
        if mapping_keys(text, &[BLOCK]).iter().any(|key| key == name) {
            return false;
        }
        let new_text =
            write_rendered_entry_at(text, &[BLOCK], name, &render::render_value(placeholder));
        manifest.set_text(new_text);
    } else {
        let block = format!(
            "{BLOCK}:\n  {}: {}\n",
            render::render_value(name),
            render::render_value(placeholder),
        );
        let new_text = insert_top_level_block(manifest, BLOCK, &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[BLOCK.to_string()]);
    }
    manifest
        .allow_builds
        .get_or_insert_with(IndexMap::new)
        .insert(name.to_string(), AllowBuildValue::String(placeholder.to_string()));
    true
}

pub(super) fn render_bool(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Drop undecided placeholder entries whose package is provably absent from
/// `resolved`. Explicit decisions, keys with no provable package name, and
/// entries for still-resolved packages always stay.
pub(crate) fn prune_allow_builds(
    manifest: &mut Manifest,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    const BLOCK: &str = "allowBuilds";
    let Some(allow_builds) = manifest.allow_builds.as_ref() else {
        return false;
    };
    let prunable = prunable_allow_build_keys(allow_builds, resolved);
    if prunable.is_empty() {
        return false;
    }

    // The decoded map came from this same text, so an empty key list means
    // the narrow re-parse failed; without it surviving entries can't be
    // told apart from prunable ones, so the block must stay untouched.
    let all_keys = allow_builds_keys_in_text(manifest.text());
    if all_keys.is_empty() {
        return false;
    }

    if all_keys.iter().all(|key| prunable.contains(key)) {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.allow_builds = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }

    let Some(new_text) = text_without_allow_builds(manifest.text(), BLOCK, &prunable, &all_keys)
    else {
        return false;
    };
    if let Some(builds) = manifest.allow_builds.as_mut() {
        for key in &prunable {
            builds.shift_remove(key);
        }
    }
    manifest.set_text(new_text);
    true
}

/// The undecided entries whose package the workspace no longer resolves. A
/// decided entry is the user's answer and always stays.
fn prunable_allow_build_keys(
    allow_builds: &IndexMap<String, AllowBuildValue>,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> HashSet<String> {
    allow_builds
        .iter()
        .filter_map(|(key, value)| {
            let AllowBuildValue::String(val) = value else {
                return None;
            };
            if val != crate::UNDECIDED_ALLOW_BUILD {
                return None;
            }
            let name = allow_build_key_package_name(key)?;
            (!resolved.contains_key(name)).then(|| key.clone())
        })
        .collect()
}

/// The document with the prunable entries removed, or `None` when the text
/// and the decoded map disagree and the block has to stay untouched.
fn text_without_allow_builds(
    text: &str,
    block: &str,
    prunable: &HashSet<String>,
    all_keys: &[String],
) -> Option<String> {
    let entries = match locate_mapping(text, &[block]) {
        Inline::Flow(collection) => {
            let prunable: Vec<String> = prunable.iter().cloned().collect();
            return Some(flow::remove_keys(text, &collection, &prunable));
        }
        Inline::Unsupported => return None,
        Inline::Block => locate(text, &[block]).map(|mapping| mapping.entries)?,
    };
    // Entries are removed by pairing each text line with its decoded key —
    // the raw key text can differ from the decoded form (quoting, escapes).
    // A count mismatch means the two views disagree (e.g. duplicate keys),
    // so leave the block untouched.
    if entries.is_empty() || entries.len() != all_keys.len() {
        return None;
    }
    let mut out = text.to_string();
    for (entry, key) in entries.iter().zip(all_keys).rev() {
        if prunable.contains(key) {
            out.replace_range(entry.line_start..entry.block_end, "");
        }
    }
    Some(out)
}

/// The package name an `allowBuilds` key identifies — the key itself for a
/// bare name, the name half of a `name@version` dep-path key — or `None`
/// for keys carrying no single package name (hashless git-repo keys,
/// malformed shapes). The key shapes mirror
/// `allow_build_key_from_ignored_build` in the deps-restorer crate, which
/// this crate cannot depend on.
fn allow_build_key_package_name(key: &str) -> Option<&str> {
    if !key.contains('#') && (key.starts_with("git+") || key.contains("@git+")) {
        return None;
    }
    let name = match key.get(1..).and_then(|rest| rest.find('@')) {
        // The version part after the `@` separator must be non-empty.
        Some(off) if off + 2 < key.len() => &key[..=off],
        Some(_) => return None,
        None => key,
    };
    (!name.is_empty() && !name.contains(':')).then_some(name)
}

fn allow_builds_keys_in_text(text: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct OnlyAllowBuilds {
        #[serde(default, rename = "allowBuilds")]
        allow_builds: Option<IndexMap<String, serde::de::IgnoredAny>>,
    }
    serde_saphyr::from_str::<OnlyAllowBuilds>(text)
        .ok()
        .and_then(|parsed| parsed.allow_builds)
        .map(|map| map.into_keys().collect())
        .unwrap_or_default()
}
