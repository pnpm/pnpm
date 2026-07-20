//! The catalog merge + format-preserving edit pass.
//!
//! Merges a set of updated catalogs into a workspace manifest's catalog
//! blocks. Because the merge only ever inserts new entries/blocks or updates a
//! single value (existing entries never move relative to each other), the
//! format-preserving edits are expressed as targeted text splices for inserts
//! and a [`yamlpatch`] `Op::Replace` for value updates.

use std::fmt::Write as _;

use indexmap::IndexMap;
use pacquet_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use yamlpatch::{Op, Patch};
use yamlpath::{Component, Document, Route};

use crate::{model::Manifest, render};

/// Merge `updated` into `manifest`'s catalog blocks. Returns whether anything
/// changed.
pub(crate) fn add_catalogs(
    manifest: &mut Manifest,
    updated: &Catalogs,
) -> Result<bool, Box<yamlpatch::Error>> {
    let mut changed = false;
    for (catalog_name, entries) in updated {
        if entries.is_empty() {
            continue;
        }
        for (dep, specifier) in entries {
            changed |= upsert(manifest, catalog_name, dep, specifier)?;
        }
    }
    Ok(changed)
}

/// Upsert one `name → specifier` entry into the top-level
/// `configDependencies:` block, creating the block if absent. Returns
/// whether anything changed. The entry value is a clean specifier; the
/// resolved integrity lives in the env lockfile, so this only ever
/// writes the `configDependencies` map in `pnpm-workspace.yaml`.
pub(crate) fn add_config_dependency(
    manifest: &mut Manifest,
    name: &str,
    specifier: &str,
) -> Result<bool, Box<yamlpatch::Error>> {
    const BLOCK: &str = "configDependencies";
    let current_matches =
        manifest.config_dependencies.as_ref().and_then(|deps| deps.get(name)).map(String::as_str)
            == Some(specifier);
    let changed = upsert_top_level_entry(manifest, BLOCK, name, specifier, current_matches)?;
    if changed {
        manifest
            .config_dependencies
            .get_or_insert_with(IndexMap::new)
            .insert(name.to_string(), specifier.to_string());
    }
    Ok(changed)
}

/// Upsert `patchedDependencies:` entries into the workspace manifest,
/// creating the block when needed.
pub(crate) fn add_patched_dependencies(
    manifest: &mut Manifest,
    patched_dependencies: &IndexMap<String, String>,
) -> Result<bool, Box<yamlpatch::Error>> {
    const BLOCK: &str = "patchedDependencies";
    let mut changed = false;

    if patched_dependencies.is_empty() {
        let has_block = manifest.top_level_keys.iter().any(|key| key == BLOCK);
        if manifest.patched_dependencies.is_none() && !has_block {
            return Ok(false);
        }
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.patched_dependencies = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return Ok(true);
    }

    if let Some(existing) = manifest.patched_dependencies.as_ref() {
        let omitted: Vec<String> = existing
            .keys()
            .filter(|key| !patched_dependencies.contains_key(*key))
            .cloned()
            .collect();
        if !omitted.is_empty() {
            manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &omitted));
            let current = manifest
                .patched_dependencies
                .as_mut()
                .expect("existing patched dependencies should remain decoded");
            for key in &omitted {
                current.shift_remove(key);
            }
            changed = true;
        }
    }

    for (key, path) in patched_dependencies {
        let current_matches = manifest
            .patched_dependencies
            .as_ref()
            .and_then(|deps| deps.get(key))
            .map(String::as_str)
            == Some(path);
        let entry_changed = upsert_top_level_entry(manifest, BLOCK, key, path, current_matches)?;
        if entry_changed {
            manifest
                .patched_dependencies
                .get_or_insert_with(IndexMap::new)
                .insert(key.clone(), path.clone());
            changed = true;
        }
    }
    Ok(changed)
}

/// Upsert one `selector → specifier` entry into the top-level `overrides:`
/// block, creating the block if absent. Returns whether anything changed.
/// Used by `pacquet link` and (one entry at a time) by `pnpm audit --fix`.
pub(crate) fn add_overrides(
    manifest: &mut Manifest,
    selector: &str,
    specifier: &str,
) -> Result<bool, Box<yamlpatch::Error>> {
    const BLOCK: &str = "overrides";
    let current_matches =
        manifest.overrides.as_ref().and_then(|deps| deps.get(selector)).map(String::as_str)
            == Some(specifier);
    let changed = upsert_top_level_entry(manifest, BLOCK, selector, specifier, current_matches)?;
    if changed {
        manifest
            .overrides
            .get_or_insert_with(IndexMap::new)
            .insert(selector.to_string(), specifier.to_string());
    }
    Ok(changed)
}

/// Delete the given `selectors` from the top-level `overrides:` block,
/// dropping the whole block when nothing remains. Selectors absent from the
/// block are ignored. Returns whether anything changed. The inverse of
/// [`add_overrides`]; used by `pacquet unlink`.
pub(crate) fn remove_overrides(manifest: &mut Manifest, selectors: &[String]) -> bool {
    const BLOCK: &str = "overrides";
    let present: Vec<String> = match manifest.overrides.as_ref() {
        Some(overrides) => {
            selectors.iter().filter(|selector| overrides.contains_key(*selector)).cloned().collect()
        }
        None => return false,
    };
    if present.is_empty() {
        return false;
    }

    // Emptiness is judged from the keys actually in the YAML, not the decoded
    // map: `Manifest::parse` drops non-string override values, so the decoded
    // map can be empty while the block still holds other entries. Deleting the
    // whole block off the decoded map would silently drop that configuration.
    let all_keys = override_keys_in_text(manifest.text());
    let remaining_keys: Vec<&String> =
        all_keys.iter().filter(|key| !present.contains(key)).collect();

    if let Some(overrides) = manifest.overrides.as_mut() {
        for selector in &present {
            overrides.shift_remove(selector);
        }
    }

    if remaining_keys.is_empty() {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.overrides = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }

    // A block-style mapping stores each entry on its own line, so the requested
    // entries can be excised surgically while every other entry (string or not)
    // is preserved. A flow-style mapping (`overrides: { ... }`) exposes no line
    // entries, so it can only be rewritten wholesale — which the decoded map can
    // do faithfully only when it accounts for every remaining key (i.e. the
    // block has no non-string entries). When it does not, leave the file
    // untouched rather than drop the entries we cannot reserialize.
    let line_based =
        locate(manifest.text(), &[BLOCK]).is_some_and(|mapping| !mapping.entries.is_empty());

    if line_based {
        manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &present));
        true
    } else if manifest.overrides.as_ref().map_or(0, IndexMap::len) == remaining_keys.len() {
        rerender_overrides_block(manifest, BLOCK);
        true
    } else {
        false
    }
}

/// Raw dependency specifiers per package name, collected from every
/// workspace project manifest plus the workspace manifest's own
/// `overrides:` values. The upstream `packageReferences` map: a catalog
/// entry survives the cleanup pass only when this map holds a
/// `catalog:` reference for its package.
pub(crate) type CatalogReferences =
    std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

/// The `cleanupUnusedCatalogs` pass: drop catalog entries that no
/// collected reference names. A default-catalog entry survives only via
/// a bare `catalog:` reference; a named-catalog entry survives via
/// `catalog:<name>` or bare `catalog:`. Emptied blocks are dropped
/// whole. Returns whether anything changed.
///
/// A flow-style (`catalog: { ... }`) block exposes no line entries for
/// the partial-removal splice, so its entries are left untouched — the
/// same conservative stance [`remove_overrides`] takes.
pub(crate) fn remove_unused_catalogs(
    manifest: &mut Manifest,
    references: &CatalogReferences,
) -> bool {
    remove_unused_default_catalog(manifest, references)
        | remove_unused_named_catalogs(manifest, references)
}

fn is_referenced(references: &CatalogReferences, pkg: &str, specs: &[&str]) -> bool {
    references.get(pkg).is_some_and(|refs| specs.iter().any(|spec| refs.contains(*spec)))
}

fn remove_unused_default_catalog(manifest: &mut Manifest, references: &CatalogReferences) -> bool {
    const BLOCK: &str = "catalog";
    let Some(catalog) = manifest.catalog.as_ref() else { return false };
    let to_remove: Vec<String> = catalog
        .keys()
        .filter(|pkg| !is_referenced(references, pkg, &["catalog:"]))
        .cloned()
        .collect();
    if to_remove.len() == catalog.len() {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.catalog = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }
    if to_remove.is_empty() || !has_line_entries(manifest.text(), &[BLOCK]) {
        return false;
    }
    manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &to_remove));
    let catalog = manifest.catalog.as_mut().expect("catalog presence checked above");
    for pkg in &to_remove {
        catalog.shift_remove(pkg);
    }
    true
}

fn remove_unused_named_catalogs(manifest: &mut Manifest, references: &CatalogReferences) -> bool {
    const BLOCK: &str = "catalogs";
    let Some(catalogs) = manifest.catalogs.as_ref() else { return false };
    let mut names_to_drop: Vec<String> = Vec::new();
    let mut entry_removals: Vec<(String, Vec<String>)> = Vec::new();
    for (name, entries) in catalogs {
        let scoped = format!("catalog:{name}");
        let to_remove: Vec<String> = entries
            .keys()
            .filter(|pkg| !is_referenced(references, pkg, &[scoped.as_str(), "catalog:"]))
            .cloned()
            .collect();
        if to_remove.len() == entries.len() {
            names_to_drop.push(name.clone());
        } else if !to_remove.is_empty() {
            entry_removals.push((name.clone(), to_remove));
        }
    }

    let mut changed = false;
    for (name, to_remove) in &entry_removals {
        if !has_line_entries(manifest.text(), &[BLOCK, name]) {
            continue;
        }
        manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK, name], to_remove));
        let entries = manifest
            .catalogs
            .as_mut()
            .and_then(|catalogs| catalogs.get_mut(name))
            .expect("named catalog presence checked above");
        for pkg in to_remove {
            entries.shift_remove(pkg);
        }
        changed = true;
    }

    let total_names = manifest.catalogs.as_ref().map_or(0, IndexMap::len);
    if names_to_drop.len() == total_names {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.catalogs = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }
    if !names_to_drop.is_empty() && has_line_entries(manifest.text(), &[BLOCK]) {
        manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &names_to_drop));
        let catalogs = manifest.catalogs.as_mut().expect("catalogs presence checked above");
        for name in &names_to_drop {
            catalogs.shift_remove(name);
        }
        changed = true;
    }
    changed
}

/// Whether the mapping at `path` is written in block style — i.e. it has
/// per-line entries the removal splices can excise.
fn has_line_entries(text: &str, path: &[&str]) -> bool {
    locate(text, path).is_some_and(|mapping| !mapping.entries.is_empty())
}

/// Every key under the top-level `overrides:` block as written in `text`,
/// including non-string values that the decoded [`Manifest`] drops. Returns an
/// empty list when the block is absent or the text does not parse.
fn override_keys_in_text(text: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct OnlyOverrides {
        #[serde(default)]
        overrides: Option<IndexMap<String, serde::de::IgnoredAny>>,
    }
    serde_saphyr::from_str::<OnlyOverrides>(text)
        .ok()
        .and_then(|parsed| parsed.overrides)
        .map(|map| map.into_keys().collect())
        .unwrap_or_default()
}

/// Set `auditConfig.ignoreGhsas:` to `ghsas` (the complete desired list),
/// creating the `auditConfig:` block or the nested `ignoreGhsas:` key when
/// absent. An empty `ghsas` removes the `auditConfig:` block. `pnpm audit
/// --ignore` calls this with the merged ignore list. Returns whether anything
/// changed.
pub(crate) fn set_audit_ignore_ghsas(
    manifest: &mut Manifest,
    ghsas: &[String],
) -> Result<bool, Box<yamlpatch::Error>> {
    const BLOCK: &str = "auditConfig";
    let current = manifest.audit_ignore_ghsas.as_deref().unwrap_or_default();

    if ghsas.is_empty() {
        let text = manifest.text();
        let Some(mapping) = locate(text, &[BLOCK]) else {
            return Ok(false);
        };
        // Nothing to remove if `ignoreGhsas` isn't present — and crucially,
        // don't touch sibling `auditConfig` keys.
        if !mapping.entries.iter().any(|entry| entry.key == "ignoreGhsas") {
            return Ok(false);
        }
        let only_ignore_ghsas = mapping.entries.iter().all(|entry| entry.key == "ignoreGhsas");
        if only_ignore_ghsas {
            manifest.set_text(remove_top_level_block(text, BLOCK));
            manifest.top_level_keys.retain(|key| key != BLOCK);
        } else {
            manifest.set_text(remove_mapping_entries(text, &[BLOCK], &["ignoreGhsas".to_string()]));
        }
        manifest.audit_ignore_ghsas = None;
        return Ok(true);
    }

    if current == ghsas {
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

/// Set the top-level `minimumReleaseAgeExclude:` block to `items` (the
/// complete desired list), creating or replacing it, and removing it when
/// `items` is empty. The caller is responsible for merging with the existing
/// entries (via `pacquet_config::version_policy::merge_package_version_specs`)
/// before calling. Returns whether anything changed.
pub(crate) fn set_minimum_release_age_excludes(manifest: &mut Manifest, items: &[String]) -> bool {
    const BLOCK: &str = "minimumReleaseAgeExclude";
    let current = manifest.minimum_release_age_exclude.as_deref().unwrap_or_default();

    if items.is_empty() {
        let has_block = manifest.top_level_keys.iter().any(|key| key == BLOCK);
        if !has_block {
            return false;
        }
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.minimum_release_age_exclude = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }

    if current == items {
        return false;
    }

    let text = manifest.text();
    let rendered = render_top_level_sequence(BLOCK, items);
    if let Some(span) = top_level_span(text, BLOCK) {
        // Preserve a trailing blank line before the next block, since the
        // span includes it but the freshly rendered block does not.
        let had_trailing_blank = text[span.key_line_start..span.block_end].ends_with("\n\n");
        let mut out = text.to_string();
        let replacement = if had_trailing_blank { format!("{rendered}\n") } else { rendered };
        out.replace_range(span.key_line_start..span.block_end, &replacement);
        manifest.set_text(out);
    } else {
        let new_text = insert_top_level_block(manifest, BLOCK, &rendered);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[BLOCK.to_string()]);
    }
    manifest.minimum_release_age_exclude = Some(items.to_vec());
    true
}

/// Render a top-level block whose value is a block sequence (`key:` then
/// `  - item` lines).
fn render_top_level_sequence(key: &str, items: &[String]) -> String {
    let mut block = String::new();
    block.push_str(key);
    block.push_str(":\n");
    for item in items {
        block.push_str("  - ");
        block.push_str(&render::render_value(item));
        block.push('\n');
    }
    block
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

/// Upsert a `key:` entry whose value is a block sequence (`items`) into the
/// existing top-level mapping `block_name`, creating or replacing the entry
/// in the position the reorder pass would choose. The mapping at
/// `block_name` must already exist. Used to write `auditConfig.ignoreGhsas`.
fn upsert_sequence_entry(text: &str, block_name: &str, key: &str, items: &[String]) -> String {
    let mapping = locate(text, &[block_name]).expect("block exists");
    let item_indent = mapping.entry_indent + 2;
    let mut rendered = String::new();
    rendered.push_str(&" ".repeat(mapping.entry_indent));
    rendered.push_str(&render::render_value(key));
    rendered.push_str(":\n");
    for item in items {
        rendered.push_str(&" ".repeat(item_indent));
        rendered.push_str("- ");
        rendered.push_str(&render::render_value(item));
        rendered.push('\n');
    }

    if let Some(entry) = mapping.entries.iter().find(|entry| entry.key == key) {
        let mut out = text.to_string();
        out.replace_range(entry.line_start..entry.block_end, &rendered);
        return out;
    }

    let existing: Vec<String> = mapping.entries.iter().map(|entry| entry.key.clone()).collect();
    let order = render::target_order(&existing, &[key.to_string()]);
    let position =
        order.iter().position(|order_key| order_key == key).expect("key is in the order");
    let offset = if position == 0 {
        mapping.body_start
    } else {
        let predecessor = &order[position - 1];
        mapping
            .entries
            .iter()
            .find(|entry| &entry.key == predecessor)
            .expect("predecessor entry exists")
            .block_end
    };
    splice(text, offset, &rendered)
}

/// Replace the on-disk `overrides:` block with a block-style rendering of the
/// decoded (already-edited) map. Used when the original block is flow-style and
/// cannot be edited entry by entry.
fn rerender_overrides_block(manifest: &mut Manifest, block_name: &str) {
    let block = {
        let overrides = manifest.overrides.as_ref().expect("non-empty overrides above");
        let mut block = format!("{block_name}:\n");
        for (selector, specifier) in overrides {
            writeln!(
                block,
                "  {}: {}",
                render::render_value(selector),
                render::render_value(specifier),
            )
            .expect("writing to a String never fails");
        }
        block
    };
    manifest.set_text(remove_top_level_block(manifest.text(), block_name));
    manifest.top_level_keys.retain(|key| key != block_name);
    let new_text = insert_top_level_block(manifest, block_name, &block);
    manifest.set_text(new_text);
    manifest.top_level_keys =
        render::target_order(&manifest.top_level_keys, &[block_name.to_string()]);
}

fn upsert_top_level_entry(
    manifest: &mut Manifest,
    block_name: &str,
    key: &str,
    value: &str,
    current_matches: bool,
) -> Result<bool, Box<yamlpatch::Error>> {
    let text = manifest.text();
    if let Some(mapping) = locate(text, &[block_name]) {
        let new_text = if mapping.entries.iter().any(|entry| entry.key == key) {
            if current_matches {
                return Ok(false);
            }
            replace_value_at(text, &[block_name], key, value)?
        } else {
            insert_entry_at(text, &[block_name], key, value)
        };
        manifest.set_text(new_text);
    } else {
        let block = format!(
            "{block_name}:\n  {}: {}\n",
            render::render_value(key),
            render::render_value(value),
        );
        let new_text = insert_top_level_block(manifest, block_name, &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &[block_name.to_string()]);
    }
    Ok(true)
}

/// Upsert one `name → bool` entry into the top-level `allowBuilds:` block,
/// creating the block if absent. Returns whether anything changed. `pnpm
/// approve-builds` calls this with each approved package set to `true` and
/// each denied/unselected package set to `false`.
pub(crate) fn add_allow_build(manifest: &mut Manifest, name: &str, value: bool) -> bool {
    const BLOCK: &str = "allowBuilds";
    let text = manifest.text();
    let changed = if let Some(mapping) = locate(text, &[BLOCK]) {
        if mapping.entries.iter().any(|entry| entry.key == name) {
            // Already present with the same value — a true no-op, so don't
            // rewrite the file (which would bump its mtime).
            if manifest.allow_builds.as_ref().and_then(|builds| builds.get(name)) == Some(&value) {
                return false;
            }
            let new_text = replace_bool_value_at(text, &[BLOCK], name, value);
            manifest.set_text(new_text);
        } else {
            let new_text = insert_rendered_entry_at(text, &[BLOCK], name, render_bool(value));
            manifest.set_text(new_text);
        }
        true
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
    manifest.allow_builds.get_or_insert_with(IndexMap::new).insert(name.to_string(), value);
    changed
}

fn render_bool(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Set the top-level `key` to `value` (a non-null JSON value), inserting the
/// block when absent and replacing it when present. Returns whether anything
/// changed — a deep-equal current value is a no-op. Used by `pnpm config set`
/// for arbitrary `pnpm-workspace.yaml` / `config.yaml` keys.
///
/// The replace path removes the old block and re-inserts the new one at the
/// reorder position (rather than an in-place value patch), so the same code
/// handles scalar and nested-object values uniformly; sibling keys and their
/// comments are preserved.
pub(crate) fn set_top_level_field(
    manifest: &mut Manifest,
    key: &str,
    value: &serde_json::Value,
) -> bool {
    if current_top_level_value(manifest.text(), key).as_ref() == Some(value) {
        return false;
    }
    let block = render_top_level_field(key, value);
    if manifest.top_level_keys.iter().any(|existing| existing == key) {
        manifest.set_text(remove_top_level_block(manifest.text(), key));
        manifest.top_level_keys.retain(|existing| existing != key);
    }
    let new_text = insert_top_level_block(manifest, key, &block);
    manifest.set_text(new_text);
    manifest.top_level_keys = render::target_order(&manifest.top_level_keys, &[key.to_string()]);
    true
}

/// Remove the top-level `key`. Returns whether anything changed (false when the
/// key is absent). Used by `pnpm config delete` and by `pnpm config set` when
/// the cast value is null/undefined.
pub(crate) fn remove_top_level_field(manifest: &mut Manifest, key: &str) -> bool {
    if !manifest.top_level_keys.iter().any(|existing| existing == key) {
        return false;
    }
    manifest.set_text(remove_top_level_block(manifest.text(), key));
    manifest.top_level_keys.retain(|existing| existing != key);
    true
}

/// Decode the current value of top-level `key` as JSON, or `None` when the key
/// is absent or the document does not parse. Used for no-op detection.
fn current_top_level_value(text: &str, key: &str) -> Option<serde_json::Value> {
    let map: IndexMap<String, serde_json::Value> = serde_saphyr::from_str(text).ok()?;
    map.get(key).cloned()
}

/// Render a brand-new top-level block for `key: value`. Scalars render inline;
/// objects and arrays render as an indented block body via [`yaml_serde`].
fn render_top_level_field(key: &str, value: &serde_json::Value) -> String {
    let key_text = render::render_value(key);
    match value {
        serde_json::Value::String(s) => format!("{key_text}: {}\n", render::render_value(s)),
        serde_json::Value::Number(n) => format!("{key_text}: {n}\n"),
        serde_json::Value::Bool(b) => format!("{key_text}: {b}\n"),
        serde_json::Value::Null => format!("{key_text}: null\n"),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            let body =
                yaml_serde::to_string(value).expect("serializing a JSON value to YAML never fails");
            let mut out = format!("{key_text}:\n");
            for line in body.trim_end_matches('\n').lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
            out
        }
    }
}

/// Where a catalog's entries live (or should be created) in the manifest.
enum Target {
    /// The top-level `catalog:` shorthand for the default catalog.
    Shorthand,
    /// A named catalog `catalogs.<name>` (including an explicit `default`).
    Named(String),
}

impl Target {
    /// The key path of the mapping holding this catalog's entries.
    fn path(&self) -> Vec<&str> {
        match self {
            Target::Shorthand => vec!["catalog"],
            Target::Named(name) => vec!["catalogs", name],
        }
    }
}

/// Insert or update one `dep → specifier` entry in `catalog_name`. Returns
/// whether the manifest changed.
fn upsert(
    manifest: &mut Manifest,
    catalog_name: &str,
    dep: &str,
    specifier: &str,
) -> Result<bool, Box<yamlpatch::Error>> {
    let is_default = catalog_name == DEFAULT_CATALOG_NAME;

    let existing_target = if is_default {
        if manifest.catalog.is_some() {
            Some(Target::Shorthand)
        } else if manifest.catalogs.as_ref().is_some_and(|c| c.contains_key(DEFAULT_CATALOG_NAME)) {
            Some(Target::Named(DEFAULT_CATALOG_NAME.to_string()))
        } else {
            None
        }
    } else if manifest.catalogs.as_ref().is_some_and(|c| c.contains_key(catalog_name)) {
        Some(Target::Named(catalog_name.to_string()))
    } else {
        None
    };

    match existing_target {
        Some(target) => upsert_existing(manifest, &target, dep, specifier),
        None => Ok(create_target(manifest, is_default, catalog_name, dep, specifier)),
    }
}

/// Upsert into a catalog block that already exists.
fn upsert_existing(
    manifest: &mut Manifest,
    target: &Target,
    dep: &str,
    specifier: &str,
) -> Result<bool, Box<yamlpatch::Error>> {
    let current = target_map(manifest, target).get(dep).cloned();
    match current {
        Some(existing) if existing == specifier => Ok(false),
        Some(_) => {
            let new_text = replace_value(manifest.text(), target, dep, specifier)?;
            manifest.set_text(new_text);
            target_map_mut(manifest, target).insert(dep.to_string(), specifier.to_string());
            Ok(true)
        }
        None => {
            let new_text = insert_entry(manifest.text(), target, dep, specifier);
            manifest.set_text(new_text);
            target_map_mut(manifest, target).insert(dep.to_string(), specifier.to_string());
            Ok(true)
        }
    }
}

/// Create a missing catalog block and write the first entry into it.
fn create_target(
    manifest: &mut Manifest,
    is_default: bool,
    catalog_name: &str,
    dep: &str,
    specifier: &str,
) -> bool {
    let value = render::render_value(specifier);
    let dep_key = render::render_value(dep);
    if is_default {
        // A new default catalog always lands in the top-level `catalog:`
        // shorthand.
        let block = format!("catalog:\n  {dep_key}: {value}\n");
        let new_text = insert_top_level_block(manifest, "catalog", &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &["catalog".to_string()]);
        manifest.catalog = Some(IndexMap::from([(dep.to_string(), specifier.to_string())]));
    } else if manifest.catalogs.is_some() {
        // `catalogs:` exists but lacks this name — add a named sub-block.
        let new_text = insert_named_subblock(manifest, catalog_name, dep, &value);
        manifest.set_text(new_text);
        manifest.catalogs.as_mut().expect("catalogs present").insert(
            catalog_name.to_string(),
            IndexMap::from([(dep.to_string(), specifier.to_string())]),
        );
    } else {
        let block = format!(
            "catalogs:\n  {}:\n    {dep_key}: {value}\n",
            render::render_value(catalog_name),
        );
        let new_text = insert_top_level_block(manifest, "catalogs", &block);
        manifest.set_text(new_text);
        manifest.top_level_keys =
            render::target_order(&manifest.top_level_keys, &["catalogs".to_string()]);
        manifest.catalogs = Some(IndexMap::from([(
            catalog_name.to_string(),
            IndexMap::from([(dep.to_string(), specifier.to_string())]),
        )]));
    }
    true
}

fn target_map<'a>(manifest: &'a Manifest, target: &Target) -> &'a IndexMap<String, String> {
    match target {
        Target::Shorthand => manifest.catalog.as_ref().expect("catalog shorthand present"),
        Target::Named(name) => manifest
            .catalogs
            .as_ref()
            .expect("catalogs present")
            .get(name)
            .expect("named catalog present"),
    }
}

fn target_map_mut<'a>(
    manifest: &'a mut Manifest,
    target: &Target,
) -> &'a mut IndexMap<String, String> {
    match target {
        Target::Shorthand => manifest.catalog.as_mut().expect("catalog shorthand present"),
        Target::Named(name) => manifest
            .catalogs
            .as_mut()
            .expect("catalogs present")
            .get_mut(name)
            .expect("named catalog present"),
    }
}

/// Replace an existing entry's value in place via [`yamlpatch`], preserving
/// the key's comments and the document's untouched bytes.
fn replace_value(
    text: &str,
    target: &Target,
    dep: &str,
    specifier: &str,
) -> Result<String, Box<yamlpatch::Error>> {
    replace_value_at(text, &target.path(), dep, specifier)
}

/// [`replace_value`] addressed by an explicit mapping path rather than a
/// catalog [`Target`], so non-catalog blocks (e.g. `configDependencies`)
/// can reuse the same comment-preserving splice.
fn replace_value_at(
    text: &str,
    path: &[&str],
    dep: &str,
    specifier: &str,
) -> Result<String, Box<yamlpatch::Error>> {
    replace_scalar_at(text, path, dep, yaml_serde::Value::from(specifier))
}

/// [`replace_value_at`] for an arbitrary scalar value, so non-string blocks
/// (e.g. `allowBuilds`'s booleans) can reuse the same comment-preserving
/// splice.
fn replace_scalar_at(
    text: &str,
    path: &[&str],
    dep: &str,
    value: yaml_serde::Value,
) -> Result<String, Box<yamlpatch::Error>> {
    let document =
        Document::new(text.to_string()).map_err(yamlpatch::Error::from).map_err(Box::new)?;
    let components: Vec<Component> = path
        .iter()
        .copied()
        .chain(std::iter::once(dep))
        .map(|key| Component::Key(key.into()))
        .collect();
    let patch = Patch { route: Route::from(components), operation: Op::Replace(value) };
    let patched = yamlpatch::apply_yaml_patches(&document, &[patch]).map_err(Box::new)?;
    Ok(patched.source().to_string())
}

/// Insert a new `dep: value` entry into an existing catalog mapping at the
/// position the reorder pass would choose (sorted-in when the block is
/// sorted, appended otherwise).
fn insert_entry(text: &str, target: &Target, dep: &str, specifier: &str) -> String {
    insert_entry_at(text, &target.path(), dep, specifier)
}

/// [`insert_entry`] addressed by an explicit mapping path, so non-catalog
/// blocks (e.g. `configDependencies`) can reuse the reorder-aware splice.
fn insert_entry_at(text: &str, path: &[&str], dep: &str, specifier: &str) -> String {
    insert_rendered_entry_at(text, path, dep, &render::render_value(specifier))
}

/// [`insert_entry_at`] for an already-rendered value text, so non-string
/// blocks (e.g. `allowBuilds`'s `true` / `false`) can reuse the
/// reorder-aware splice without going through [`render::render_value`].
fn insert_rendered_entry_at(text: &str, path: &[&str], dep: &str, value_text: &str) -> String {
    let mapping = locate(text, path).expect("mapping exists");
    let existing: Vec<String> = mapping.entries.iter().map(|entry| entry.key.clone()).collect();
    let order = render::target_order(&existing, &[dep.to_string()]);
    let position = order.iter().position(|key| key == dep).expect("dep is in the merged order");

    let line = format!(
        "{}{}: {}\n",
        " ".repeat(mapping.entry_indent),
        render::render_value(dep),
        value_text,
    );
    let offset = if position == 0 {
        mapping.body_start
    } else {
        let predecessor = &order[position - 1];
        mapping
            .entries
            .iter()
            .find(|entry| &entry.key == predecessor)
            .expect("predecessor entry exists")
            .line_end
    };
    splice(text, offset, &line)
}

fn remove_mapping_entries(text: &str, path: &[&str], keys: &[String]) -> String {
    let Some(mapping) = locate(text, path) else {
        return text.to_string();
    };
    let mut out = text.to_string();
    for entry in mapping.entries.iter().rev().filter(|entry| keys.contains(&entry.key)) {
        out.replace_range(entry.line_start..entry.block_end, "");
    }
    out
}

fn remove_top_level_block(text: &str, key: &str) -> String {
    let Some(span) = top_level_span(text, key) else {
        return text.to_string();
    };
    let mut out = text.to_string();
    out.replace_range(span.key_line_start..span.block_end, "");
    out
}

/// Insert a new named catalog (`<name>:` + its first entry) into an existing
/// top-level `catalogs:` block, at the position the reorder pass would choose.
fn insert_named_subblock(manifest: &Manifest, name: &str, dep: &str, value: &str) -> String {
    let text = manifest.text();
    let catalogs = locate(text, &["catalogs"]).expect("catalogs block exists");
    let existing: Vec<String> = catalogs.entries.iter().map(|entry| entry.key.clone()).collect();
    let order = render::target_order(&existing, &[name.to_string()]);
    let position = order.iter().position(|key| key == name).expect("name is in the merged order");

    let indent = " ".repeat(catalogs.entry_indent);
    let block = format!(
        "{indent}{}:\n{indent}  {}: {value}\n",
        render::render_value(name),
        render::render_value(dep),
    );
    let offset = if position == 0 {
        catalogs.body_start
    } else {
        let predecessor = &order[position - 1];
        catalogs
            .entries
            .iter()
            .find(|entry| &entry.key == predecessor)
            .expect("predecessor named catalog exists")
            .block_end
    };
    splice(text, offset, &block)
}

/// Insert a brand-new top-level block (`block_text`, ending in a newline) at
/// the position the reorder + blank-line passes would choose.
fn insert_top_level_block(manifest: &Manifest, new_key: &str, block_text: &str) -> String {
    let text = manifest.text();
    let order = render::target_order(&manifest.top_level_keys, &[new_key.to_string()]);
    let position =
        order.iter().position(|key| key == new_key).expect("new key is in the merged order");
    let blank_style = uses_blank_line_style(text, &manifest.top_level_keys);

    if position == 0 {
        // New key sorts to the front: prepend the block. Under blank-line
        // style the demoted original-first key gains a blank line before it.
        let separator = if blank_style && !manifest.top_level_keys.is_empty() { "\n" } else { "" };
        return format!("{block_text}{separator}{text}");
    }

    let successor = order.get(position + 1);
    if let Some(successor_key) = successor {
        let span = top_level_span(text, successor_key).expect("successor block exists");
        // Insert before the successor's key line; its existing preceding
        // blank line (if any) becomes the blank before the new block, and
        // a trailing blank line is added when the document uses that style.
        let trailing = if blank_style { "\n" } else { "" };
        splice(text, span.key_line_start, &format!("{block_text}{trailing}"))
    } else {
        // Append at the end of the document.
        let mut out = String::with_capacity(text.len() + block_text.len() + 1);
        out.push_str(text);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if blank_style && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(block_text);
        out
    }
}

fn splice(text: &str, offset: usize, insertion: &str) -> String {
    let mut out = String::with_capacity(text.len() + insertion.len());
    out.push_str(&text[..offset]);
    out.push_str(insertion);
    out.push_str(&text[offset..]);
    out
}

// ---------------------------------------------------------------------------
// Line-oriented scanning of the block-style YAML pnpm writes.
// ---------------------------------------------------------------------------

/// A located mapping and its direct child entries.
struct Mapping {
    /// Byte offset where the mapping's body (its child lines) begins.
    body_start: usize,
    /// Indentation (in spaces) of the mapping's direct child entries.
    entry_indent: usize,
    /// Direct child key lines, in document order.
    entries: Vec<EntryPos>,
}

/// One direct child entry of a mapping.
struct EntryPos {
    key: String,
    /// Byte offset where this entry's line begins.
    line_start: usize,
    /// Byte offset just past this entry's line (after its newline).
    line_end: usize,
    /// Byte offset where this entry's whole sub-block ends (for nested maps).
    block_end: usize,
}

/// Span of a top-level block keyed by `key`.
struct TopLevelSpan {
    key_line_start: usize,
    block_end: usize,
}

struct Line<'a> {
    start: usize,
    /// Content without the trailing newline.
    content: &'a str,
    /// Byte offset just past the line, including its newline.
    end: usize,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        out.push(Line { start: offset, content, end: offset + raw.len() });
        offset += raw.len();
    }
    out
}

/// Indentation of a structural line, or `None` for blank and comment lines
/// (which don't terminate a block and aren't entries).
fn structural_indent(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start().len();
    let rest = &content[indent..];
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }
    Some(indent)
}

/// The mapping-key a structural line declares (`key:` or `key: value`), if any.
///
/// The key/value delimiter is the first `:` that ends the line or is followed
/// by whitespace — a `:` inside the value, or inside a key (quoted or not,
/// e.g. an `allowBuilds` artifact key like `foo@https://example.com/foo.tgz`),
/// is not the delimiter. Splitting on the first `:` would truncate such keys.
fn line_key(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    let delimiter = structural_colon_index(trimmed)?;
    let key = trimmed[..delimiter].trim_end();
    if key.is_empty() {
        return None;
    }
    Some(strip_quotes(key))
}

/// Byte offset of the YAML key/value delimiter in `line`: the first `:` that
/// ends the line or is followed by whitespace. A `:` inside a value or key
/// (e.g. `foo@https://...`) is not the delimiter.
fn structural_colon_index(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len())
        .find(|&idx| bytes[idx] == b':' && bytes.get(idx + 1).is_none_or(u8::is_ascii_whitespace))
}

/// Rewrite the scalar value of `key`'s existing entry under `path` in place,
/// preserving the key's text/quoting and any trailing comment. Used for
/// `allowBuilds` instead of the `yamlpatch` route, which rejects a key
/// containing `:` (an artifact pkgId such as `foo@https://example.com/foo.tgz`).
fn replace_bool_value_at(text: &str, path: &[&str], key: &str, value: bool) -> String {
    let mapping = locate(text, path).expect("mapping exists");
    let entry = mapping.entries.iter().find(|entry| entry.key == key).expect("entry exists");
    let line = &text[entry.line_start..entry.line_end];
    let content = line.strip_suffix('\n').unwrap_or(line);
    let indent_len = content.len() - content.trim_start().len();
    let colon = indent_len
        + structural_colon_index(&content[indent_len..]).expect("entry line has a delimiter");
    let key_text = content[..colon].trim_end();
    // Preserve any trailing comment after the value token.
    let after = content[colon + 1..].trim_start();
    let value_end = after.find(char::is_whitespace).unwrap_or(after.len());
    let trailing = &after[value_end..];
    let new_line = format!("{key_text}: {}{trailing}\n", render_bool(value));

    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..entry.line_start]);
    out.push_str(&new_line);
    out.push_str(&text[entry.line_end..]);
    out
}

fn strip_quotes(key: &str) -> String {
    let bytes = key.as_bytes();
    if key.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        key[1..key.len() - 1].to_string()
    } else {
        key.to_string()
    }
}

/// Locate the mapping reached by following `path` from the document root.
fn locate(text: &str, path: &[&str]) -> Option<Mapping> {
    let all = lines(text);
    let mut lo = 0usize;
    let mut hi = all.len();
    let mut base_indent = 0usize;

    for (depth, segment) in path.iter().enumerate() {
        let key_idx = (lo..hi).find(|&idx| {
            structural_indent(all[idx].content) == Some(base_indent)
                && line_key(all[idx].content).as_deref() == Some(*segment)
        })?;
        // The block ends at the next structural line indented at or below
        // `base_indent`.
        let block_end_idx = ((key_idx + 1)..hi)
            .find(|&idx| {
                structural_indent(all[idx].content).is_some_and(|indent| indent <= base_indent)
            })
            .unwrap_or(hi);

        // The child indent is whatever the block's first structural line
        // uses, not a hard-coded two spaces — so a manifest written with a
        // wider indent is still traversed correctly.
        let child_indent = (key_idx + 1..block_end_idx)
            .find_map(|idx| structural_indent(all[idx].content))
            .unwrap_or(base_indent + 2);

        if depth + 1 == path.len() {
            let body_start = all.get(key_idx + 1).map_or(all[key_idx].end, |line| line.start);
            let entries = collect_entries(&all, key_idx + 1, block_end_idx, child_indent);
            return Some(Mapping { body_start, entry_indent: child_indent, entries });
        }

        lo = key_idx + 1;
        hi = block_end_idx;
        base_indent = child_indent;
    }
    None
}

/// Collect the direct child entries (key lines at `entry_indent`) within
/// `[from, to)`, recording where each entry's own sub-block ends.
fn collect_entries(all: &[Line<'_>], from: usize, to: usize, entry_indent: usize) -> Vec<EntryPos> {
    let mut entries = Vec::new();
    let mut idx = from;
    while idx < to {
        if structural_indent(all[idx].content) == Some(entry_indent)
            && let Some(key) = line_key(all[idx].content)
        {
            let block_end_idx = ((idx + 1)..to)
                .find(|&next| {
                    structural_indent(all[next].content)
                        .is_some_and(|indent| indent <= entry_indent)
                })
                .unwrap_or(to);
            let block_end = all.get(block_end_idx).map_or(all[to - 1].end, |line| line.start);
            entries.push(EntryPos {
                key,
                line_start: all[idx].start,
                line_end: all[idx].end,
                block_end,
            });
            idx = block_end_idx;
        } else {
            idx += 1;
        }
    }
    entries
}

/// Whether the top-level `key:` carries an inline value (a flow mapping /
/// flow sequence / scalar) on the same line rather than a block body on the
/// following lines. The block-style splice writers (e.g.
/// [`set_audit_ignore_ghsas`]) assume a block body, so a caller can refuse an
/// inline shape instead of corrupting it. A bare `key:` (optionally with a
/// trailing comment) is block-style and returns `false`.
pub(crate) fn top_level_has_inline_value(text: &str, key: &str) -> bool {
    for line in text.lines() {
        if structural_indent(line) != Some(0) || line_key(line).as_deref() != Some(key) {
            continue;
        }
        let trimmed = line.trim_start();
        let Some(colon) = structural_colon_index(trimmed) else { return false };
        let after = trimmed[colon + 1..].trim_start();
        return !after.is_empty() && !after.starts_with('#');
    }
    false
}

/// The starting offset of a top-level key's line.
fn top_level_span(text: &str, key: &str) -> Option<TopLevelSpan> {
    let all = lines(text);
    let key_idx = all.iter().position(|line| {
        structural_indent(line.content) == Some(0) && line_key(line.content).as_deref() == Some(key)
    })?;
    let next_key_idx = ((key_idx + 1)..all.len())
        .find(|&idx| structural_indent(all[idx].content) == Some(0))
        .unwrap_or(all.len());
    let block_end_idx = leading_comment_start(&all, key_idx + 1, next_key_idx);
    let block_end = all
        .get(block_end_idx)
        .map_or_else(|| all.last().map_or(0, |line| line.end), |line| line.start);
    all.get(key_idx)
        .filter(|line| {
            structural_indent(line.content) == Some(0)
                && line_key(line.content).as_deref() == Some(key)
        })
        .map(|line| TopLevelSpan { key_line_start: line.start, block_end })
}

fn leading_comment_start(all: &[Line<'_>], block_start: usize, next_key_idx: usize) -> usize {
    if next_key_idx == all.len() {
        return next_key_idx;
    }
    let mut idx = next_key_idx;
    while idx > block_start && is_comment_line(all[idx - 1].content) {
        idx -= 1;
    }
    idx
}

fn is_comment_line(content: &str) -> bool {
    let trimmed = content.trim_start();
    !trimmed.is_empty() && trimmed.starts_with('#')
}

/// Whether every original non-first top-level key has a blank line before it.
fn uses_blank_line_style(text: &str, top_level_keys: &[String]) -> bool {
    if top_level_keys.len() < 2 {
        return false;
    }
    let all = lines(text);
    let mut non_first = 0;
    let mut non_first_with_blank = 0;
    for key in &top_level_keys[1..] {
        let Some(idx) = all.iter().position(|line| {
            structural_indent(line.content) == Some(0)
                && line_key(line.content).as_deref() == Some(key.as_str())
        }) else {
            continue;
        };
        non_first += 1;
        if has_blank_before(&all, idx) {
            non_first_with_blank += 1;
        }
    }
    non_first > 0 && non_first == non_first_with_blank
}

/// Whether a blank line precedes the key at `idx`, looking past the key's own
/// leading comment lines.
fn has_blank_before(all: &[Line<'_>], idx: usize) -> bool {
    let mut cursor = idx;
    while cursor > 0 {
        let prev = &all[cursor - 1];
        let trimmed = prev.content.trim_start();
        if trimmed.is_empty() {
            return true;
        }
        if trimmed.starts_with('#') {
            cursor -= 1;
            continue;
        }
        return false;
    }
    false
}
