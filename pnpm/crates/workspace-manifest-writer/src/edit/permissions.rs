use super::{
    Inline, Manifest, flow, locate, locate_mapping, nested::set_nested_entry,
    remove_top_level_block, render, render_bool,
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
        &[BLOCK, pkg],
        capability,
        (&render_permission_value(value), &permission_json(value)),
    )
}

/// Upsert `globalPackagesConfig.permissions.<pkg>.<capability>: <value>`.
pub(crate) fn set_global_packages_permission(
    manifest: &mut Manifest,
    pkg: &str,
    capability: &str,
    value: &PermissionValue,
) -> bool {
    set_nested_entry(
        manifest,
        &["globalPackagesConfig", BLOCK, pkg],
        capability,
        (&render_permission_value(value), &permission_json(value)),
    )
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
        manifest
            .document
            .keys
            .retain(|key| key != ALLOW_BUILDS);
        remove_top_level_block(text, ALLOW_BUILDS)
    } else if let Inline::Flow(collection) = locate_mapping(text, &[ALLOW_BUILDS]) {
        flow::remove_keys(text, &collection, &[pkg.to_string()])
    } else {
        let Some(entry) = locate(text, &[ALLOW_BUILDS]).and_then(|mapping| {
            mapping
                .entries
                .into_iter()
                .find(|entry| entry.key == pkg)
        }) else {
            return false;
        };
        let mut out = text.to_string();
        out.replace_range(entry.line_start..entry.block_end, "");
        out
    };
    manifest.document.set_text(new_text);
    true
}
