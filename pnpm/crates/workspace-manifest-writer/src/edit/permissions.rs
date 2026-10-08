use super::{
    IndexMap, Inline, Manifest, flow, insert_top_level_block, locate, locate_mapping, mapping_keys,
    remove_top_level_block, render, render_bool, replace_bool_value_at, write_named_subblock,
    write_rendered_entry_at,
};

const BLOCK: &str = "permissions";

/// Upsert `permissions.<pkg>.<capability>: <value>`, creating the block and
/// the package entry if absent, one capability per line. Returns whether
/// anything changed.
pub(crate) fn set_permission(
    manifest: &mut Manifest,
    pkg: &str,
    capability: &str,
    value: bool,
) -> bool {
    let text = manifest.document.text();
    let new_text = if locate(text, &[BLOCK]).is_none()
        && !matches!(locate_mapping(text, &[BLOCK]), Inline::Flow(_))
    {
        let block = format!(
            "{BLOCK}:\n  {}:\n    {capability}: {}\n",
            render::render_value(pkg),
            render_bool(value),
        );
        let new_text = insert_top_level_block(manifest, BLOCK, &block);
        manifest.document.keys =
            render::target_order(&manifest.document.keys, &[BLOCK.to_string()]);
        new_text
    } else if !mapping_keys(text, &[BLOCK])
        .iter()
        .any(|key| key == pkg)
    {
        write_named_subblock(text, BLOCK, pkg, capability, render_bool(value))
    } else if !mapping_keys(text, &[BLOCK, pkg])
        .iter()
        .any(|key| key == capability)
    {
        write_rendered_entry_at(text, &[BLOCK, pkg], capability, render_bool(value))
    } else if recorded_permission(text, pkg, capability) == Some(value) {
        return false;
    } else if let Inline::Flow(collection) = locate_mapping(text, &[BLOCK, pkg]) {
        flow::upsert(text, &collection, capability, render_bool(value))
    } else {
        replace_bool_value_at(text, &[BLOCK, pkg], capability, value)
    };
    manifest.document.set_text(new_text);
    true
}

fn recorded_permission(text: &str, pkg: &str, capability: &str) -> Option<bool> {
    #[derive(serde::Deserialize)]
    struct OnlyPermissions {
        #[serde(default)]
        permissions: IndexMap<String, IndexMap<String, serde_json::Value>>,
    }
    serde_saphyr::from_str::<OnlyPermissions>(text)
        .ok()?
        .permissions
        .get(pkg)?
        .get(capability)?
        .as_bool()
}

/// Remove `pkg`'s entry from the top-level `allowBuilds:` block, and the
/// block when that was its last entry. Returns whether anything changed.
pub(crate) fn remove_allow_build(manifest: &mut Manifest, pkg: &str) -> bool {
    const ALLOW_BUILDS: &str = "allowBuilds";
    let Some(allow_builds) = manifest.allow_builds.as_mut() else { return false };
    if allow_builds.shift_remove(pkg).is_none() {
        return false;
    }
    let text = manifest.document.text().to_owned();
    let text = text.as_str();
    let new_text = if allow_builds.is_empty() {
        manifest.allow_builds = None;
        manifest.document.keys.retain(|key| key != ALLOW_BUILDS);
        remove_top_level_block(text, ALLOW_BUILDS)
    } else if let Inline::Flow(collection) = locate_mapping(text, &[ALLOW_BUILDS]) {
        flow::remove_keys(text, &collection, &[pkg.to_string()])
    } else {
        let Some(entry) = locate(text, &[ALLOW_BUILDS])
            .and_then(|mapping| {
                mapping.entries
                    .into_iter()
                    .find(|entry| entry.key == pkg)
            })
        else {
            return false;
        };
        let mut out = text.to_string();
        out.replace_range(entry.line_start..entry.block_end, "");
        out
    };
    manifest.document.set_text(new_text);
    true
}
