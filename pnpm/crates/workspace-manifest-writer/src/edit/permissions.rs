use super::{
    Inline, Manifest, flow, insert_top_level_block, locate, locate_mapping, mapping_keys,
    remove_top_level_block, render, render_bool, replace_value_text_at, write_named_subblock,
    write_rendered_entry_at,
};
use crate::build_settings::PermissionValue;
use pnpm_config::SkillsPermission;

const BLOCK: &str = "permissions";

/// Upsert `permissions.<pkg>.<capability>: <value>`, creating the block and
/// the package entry if absent, one capability per line. Returns whether
/// anything changed.
pub(crate) fn set_permission(
    manifest: &mut Manifest,
    pkg: &str,
    capability: &str,
    value: &PermissionValue,
) -> bool {
    set_nested_entry(
        manifest,
        [BLOCK, pkg],
        capability,
        (&render_permission_value(value), &permission_json(value)),
    )
}

/// Upsert `globalPackagesConfig.allowBuilds.<pkg>: <allowed>` in the global
/// `config.yaml`. Returns whether anything changed.
pub(crate) fn set_global_packages_allow_build(
    manifest: &mut Manifest,
    pkg: &str,
    allowed: bool,
) -> bool {
    set_nested_entry(
        manifest,
        ["globalPackagesConfig", "allowBuilds"],
        pkg,
        (render_bool(allowed), &serde_json::Value::Bool(allowed)),
    )
}

/// Upsert `<block>.<name>.<key>: <value>`, creating the block and the named
/// mapping if absent. `value` is the rendered text and the JSON it reads
/// back as, compared against what the document records for the no-op
/// decision. Returns whether anything changed.
fn set_nested_entry(
    manifest: &mut Manifest,
    [block, name]: [&str; 2],
    key: &str,
    (value_text, value_json): (&str, &serde_json::Value),
) -> bool {
    let text = manifest.document.text();
    let new_text = if locate(text, &[block]).is_none()
        && !matches!(locate_mapping(text, &[block]), Inline::Flow(_))
    {
        let entry = format!(
            "{block}:\n  {}:\n    {}: {value_text}\n",
            render::render_value(name),
            render::render_value(key),
        );
        let new_text = insert_top_level_block(manifest, block, &entry);
        manifest.document.keys =
            render::target_order(&manifest.document.keys, &[block.to_string()]);
        new_text
    } else if !mapping_keys(text, &[block])
        .iter()
        .any(|existing| existing == name)
    {
        write_named_subblock(text, block, name, key, value_text)
    } else if !mapping_keys(text, &[block, name])
        .iter()
        .any(|existing| existing == key)
    {
        write_rendered_entry_at(text, &[block, name], key, value_text)
    } else if recorded_value(text, [block, name, key]).as_ref() == Some(value_json) {
        return false;
    } else if let Inline::Flow(collection) = locate_mapping(text, &[block, name]) {
        flow::upsert(text, &collection, key, value_text)
    } else {
        replace_value_text_at(text, &[block, name], key, value_text)
    };
    manifest.document.set_text(new_text);
    true
}

/// The entry's value text: booleans plain, a chosen skills list inline in
/// flow style (`skills: [guide, migrate]`), one line per entry, so every
/// splice above stays line-based.
fn render_permission_value(value: &PermissionValue) -> String {
    match value {
        PermissionValue::Build(allowed) => render_bool(*allowed).to_string(),
        PermissionValue::Skills(SkillsPermission::All(allowed)) => {
            render_bool(*allowed).to_string()
        }
        PermissionValue::Skills(SkillsPermission::Chosen(chosen)) => {
            format!(
                "[{}]",
                chosen
                    .iter()
                    .map(|skill| render_flow_item(skill))
                    .collect::<Vec<String>>()
                    .join(", "),
            )
        }
    }
}

/// `item` rendered for a flow sequence. A plain scalar there ends at a flow
/// indicator, and a skill name is a directory name a package chose, so one
/// holding an indicator is double-quoted.
fn render_flow_item(item: &str) -> String {
    if item.contains([',', '[', ']', '{', '}']) {
        serde_json::to_string(item).expect("serializing a string to JSON never fails")
    } else {
        render::render_value(item)
    }
}

/// The value as JSON, compared against what the document records for the
/// no-op decision.
fn permission_json(value: &PermissionValue) -> serde_json::Value {
    serde_json::to_value(value).expect("a permission value serializes to JSON")
}

fn recorded_value(text: &str, path: [&str; 3]) -> Option<serde_json::Value> {
    let document = serde_saphyr::from_str::<serde_json::Value>(text).ok()?;
    path.iter()
        .try_fold(&document, |value, key| value.get(key))
        .cloned()
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
