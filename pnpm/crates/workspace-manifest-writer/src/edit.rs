//! The catalog merge + format-preserving edit pass.
//!
//! Merges a set of updated catalogs into a workspace manifest's catalog
//! blocks. Because the merge only ever inserts new entries/blocks or updates a
//! single value (existing entries never move relative to each other), the
//! format-preserving edits are expressed as targeted text splices for inserts
//! and a [`yamlpatch`] `Op::Replace` for value updates.
//!
//! Those splices are line-oriented, so a block whose value is written
//! inline (`overrides: { foo: 1.0.0 }`) is handed to [`crate::flow`]
//! instead, and one neither can edit is reported through [`Inline`] so the
//! caller can refuse the write.

use std::collections::HashSet;

use indexmap::IndexMap;
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use yamlpatch::{Op, Patch};
use yamlpath::{Component, Document, Route};

use crate::{
    flow,
    model::{AllowBuildValue, Manifest},
    render,
};

/// How the value a key path names is written.
pub(crate) enum Inline {
    /// A block body on the following lines — or no such key at all. The
    /// line-oriented splices in this module apply.
    Block,
    /// An inline value none of the splices can edit: a multi-line flow
    /// collection (whose interleaved comments a rebuild would drop), an
    /// alias, or a scalar where a collection belongs.
    Unsupported,
    /// A flow collection written on one line, edited by [`crate::flow`].
    Flow(flow::Collection),
}

/// Classify how the mapping at `path` is written.
fn locate_mapping(text: &str, path: &[&str]) -> Inline {
    locate_inline(text, path, flow::Kind::Mapping)
}

/// Classify how the sequence at `path` is written.
fn locate_sequence(text: &str, path: &[&str]) -> Inline {
    locate_inline(text, path, flow::Kind::Sequence)
}

/// Classify how the value of `path` is written. A flow collection of a kind
/// other than `expected` is reported as unsupported: no writer here can put
/// a mapping entry into a sequence, or the reverse.
fn locate_inline(text: &str, path: &[&str], expected: flow::Kind) -> Inline {
    let Some(offset) = inline_value_start(text, path) else { return Inline::Block };
    match flow::parse(text, offset) {
        Some(collection) if collection.kind() == expected => Inline::Flow(collection),
        Some(_) | None => Inline::Unsupported,
    }
}

/// Whether the value at `path` is an inline shape no writer can edit. A
/// caller refuses the whole write rather than corrupt it.
pub(crate) fn has_unsupported_inline_value(text: &str, path: &[&str]) -> bool {
    document_root_is_inline(text)
        || (matches!(locate_mapping(text, path), Inline::Unsupported)
            && matches!(locate_sequence(text, path), Inline::Unsupported))
}

/// Whether the document itself is written as a flow collection
/// (`{ overrides: { foo: 1.0.0 } }`). Its keys are then not top-level lines
/// at all, so neither the splices here nor a new top-level block can
/// address them.
pub(crate) fn document_root_is_inline(text: &str) -> bool {
    text.lines()
        .find(|line| structural_indent(line).is_some())
        .is_some_and(|line| line.trim_start().starts_with(['{', '[']))
}

/// The keys of the mapping at `path`, whether it is written in block or
/// single-line flow style. Empty for a mapping with no entries, an
/// unsupported inline value, or an absent key.
fn mapping_keys(text: &str, path: &[&str]) -> Vec<String> {
    match locate_mapping(text, path) {
        Inline::Flow(collection) => collection.keys(),
        Inline::Unsupported => Vec::new(),
        Inline::Block => locate(text, path)
            .map(|mapping| mapping.entries.into_iter().map(|entry| entry.key).collect())
            .unwrap_or_default(),
    }
}

/// Byte offset of the value of the key `path` names, when that value sits
/// on the key's own line. `None` when the key is absent or its value is a
/// block body on the lines below.
fn inline_value_start(text: &str, path: &[&str]) -> Option<usize> {
    let (key, parent) = path.split_last()?;
    if parent.is_empty() {
        let span = top_level_span(text, key)?;
        return inline_value_on_line(text, span.key_line_start);
    }
    if let Some(entry) = locate(text, parent)
        .and_then(|mapping| mapping.entries.into_iter().find(|entry| entry.key == *key))
    {
        return inline_value_on_line(text, entry.line_start);
    }
    // The parent has no line entries of its own when it is itself written
    // inline, and then the key lives among its flow entries.
    match locate_mapping(text, parent) {
        Inline::Flow(collection) => collection.value_start(key),
        Inline::Block | Inline::Unsupported => None,
    }
}

/// Byte offset of the value written after the `key:` on the line starting
/// at `line_start`. `None` when the line carries no value (a bare `key:`,
/// optionally followed by a comment), which makes it block style.
fn inline_value_on_line(text: &str, line_start: usize) -> Option<usize> {
    let line_end = text[line_start..].find('\n').map_or(text.len(), |offset| line_start + offset);
    let content = &text[line_start..line_end];
    let indent = content.len() - content.trim_start().len();
    let colon = indent + structural_colon_index(&content[indent..])?;
    let after = &content[colon + 1..];
    let value = after.trim_start();
    if value.is_empty() || value.starts_with('#') {
        return None;
    }
    Some(line_start + colon + 1 + (after.len() - value.len()))
}

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
    let nothing_remains = all_keys.iter().all(|key| present.contains(key));

    if let Some(overrides) = manifest.overrides.as_mut() {
        for selector in &present {
            overrides.shift_remove(selector);
        }
    }

    if nothing_remains {
        manifest.set_text(remove_top_level_block(manifest.text(), BLOCK));
        manifest.overrides = None;
        manifest.top_level_keys.retain(|key| key != BLOCK);
        return true;
    }

    // Both a block-style mapping and a single-line flow one excise the
    // requested entries surgically, leaving every other entry — string or
    // not — as written. An inline shape neither can edit leaves the file
    // untouched rather than dropping what it cannot reserialize.
    if has_unsupported_inline_value(manifest.text(), &[BLOCK]) {
        return false;
    }
    manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &present));
    true
}

/// Raw dependency specifiers per package name, collected from every
/// workspace project manifest plus the workspace manifest's own
/// `overrides:` values. The upstream `packageReferences` map: a catalog
/// entry survives the cleanup pass only when this map holds a
/// `catalog:` reference for its package.
pub(crate) type CatalogReferences =
    std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

/// The `catalogPrune` pass: drop catalog entries that no
/// collected reference names. A default-catalog entry survives only via
/// a bare `catalog:` reference; a named-catalog entry survives via
/// `catalog:<name>` or bare `catalog:`. Emptied blocks are dropped
/// whole. Returns whether anything changed.
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
    if to_remove.is_empty() || !has_removable_entries(manifest.text(), &[BLOCK]) {
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
        if !has_removable_entries(manifest.text(), &[BLOCK, name]) {
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
    if !names_to_drop.is_empty() && has_removable_entries(manifest.text(), &[BLOCK]) {
        manifest.set_text(remove_mapping_entries(manifest.text(), &[BLOCK], &names_to_drop));
        let catalogs = manifest.catalogs.as_mut().expect("catalogs presence checked above");
        for name in &names_to_drop {
            catalogs.shift_remove(name);
        }
        changed = true;
    }
    changed
}

/// Whether the mapping at `path` holds entries the removal splices can
/// excise: per-line entries in block style, or the entries of a
/// single-line flow mapping.
fn has_removable_entries(text: &str, path: &[&str]) -> bool {
    !mapping_keys(text, path).is_empty()
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
        return Ok(changed);
    }

    const BLOCK: &str = "auditConfig";
    let current = manifest.audit_ignore_ghsas.as_deref().unwrap_or_default();

    if ghsas.is_empty() {
        let text = manifest.text();
        if locate(text, &[BLOCK]).is_none() {
            return Ok(false);
        }
        // Nothing to remove if `ignoreGhsas` isn't present — and crucially,
        // don't touch sibling `auditConfig` keys.
        if !mapping_keys(text, &[BLOCK]).iter().any(|key| key == "ignoreGhsas") {
            return Ok(false);
        }
        remove_block_list_key(manifest, BLOCK, "ignoreGhsas");
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

/// Set the top-level `minimumReleaseAgeExclude:` block to `items` (the
/// complete desired list), creating or replacing it, and removing it when
/// `items` is empty. The caller is responsible for merging with the existing
/// entries (via `pnpm_config::version_policy::merge_package_version_specs`)
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
    match locate_sequence(text, &[BLOCK]) {
        Inline::Flow(collection) => {
            let rendered: Vec<String> =
                items.iter().map(|item| render::render_value(item)).collect();
            manifest.set_text(flow::set_items(text, &collection, &rendered));
            manifest.minimum_release_age_exclude = Some(items.to_vec());
            return true;
        }
        // Rendering the whole block afresh would drop the comments an
        // inline value this writer cannot edit may hold, so leave it be;
        // the public writer refuses such a manifest outright.
        Inline::Unsupported => return false,
        Inline::Block => {}
    }

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

/// The `minimumReleaseAgeExcludePrune` pass: prune
/// `minimumReleaseAgeExclude:` entries against the versions the freshly
/// resolved lockfile records. The per-entry decision lives in
/// [`pnpm_config::version_policy::drop_unresolved_package_version_specs`];
/// the text edit is [`set_minimum_release_age_excludes`]'s block replace,
/// so a pruned-to-empty list drops the block and an unchanged list is a
/// no-op. Returns whether anything changed.
pub(crate) fn prune_minimum_release_age_excludes(
    manifest: &mut Manifest,
    resolved: &pnpm_config::version_policy::ResolvedPackageVersions,
) -> bool {
    let Some(current) = manifest.minimum_release_age_exclude.as_deref() else {
        return false;
    };
    let pruned =
        pnpm_config::version_policy::drop_unresolved_package_version_specs(current, resolved);
    set_minimum_release_age_excludes(manifest, &pruned)
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
    let rendered_items: Vec<String> = items.iter().map(|item| render::render_value(item)).collect();
    if let Inline::Flow(collection) = locate_sequence(text, &[block_name, key]) {
        return flow::set_items(text, &collection, &rendered_items);
    }
    if let Inline::Flow(collection) = locate_mapping(text, &[block_name]) {
        return flow::upsert(text, &collection, key, &flow::render_sequence(&rendered_items));
    }
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

fn upsert_top_level_entry(
    manifest: &mut Manifest,
    block_name: &str,
    key: &str,
    value: &str,
    current_matches: bool,
) -> Result<bool, Box<yamlpatch::Error>> {
    let text = manifest.text();
    if locate(text, &[block_name]).is_some() {
        let new_text = if mapping_keys(text, &[block_name]).iter().any(|entry| entry == key) {
            if current_matches {
                return Ok(false);
            }
            replace_value_at(text, &[block_name], key, value)?
        } else {
            write_entry_at(text, &[block_name], key, value)
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
    let changed = if locate(text, &[BLOCK]).is_some() {
        if mapping_keys(text, &[BLOCK]).iter().any(|key| key == name) {
            // Already present with the same value — a true no-op, so don't
            // rewrite the file (which would bump its mtime).
            if manifest.allow_builds.as_ref().and_then(|builds| builds.get(name))
                == Some(&AllowBuildValue::Bool(value))
            {
                return false;
            }
            let new_text = if let Inline::Flow(collection) = locate_mapping(text, &[BLOCK]) {
                flow::upsert(text, &collection, name, render_bool(value))
            } else {
                replace_bool_value_at(text, &[BLOCK], name, value)
            };
            manifest.set_text(new_text);
        } else {
            let new_text = write_rendered_entry_at(text, &[BLOCK], name, render_bool(value));
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
    manifest
        .allow_builds
        .get_or_insert_with(IndexMap::new)
        .insert(name.to_string(), AllowBuildValue::Bool(value));
    changed
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
            let new_text = write_entry(manifest.text(), target, dep, specifier);
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
        let new_text = write_named_subblock(manifest, catalog_name, dep, &value);
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
    if let Inline::Flow(collection) = locate_mapping(text, path) {
        let value_text = yaml_serde::to_string(&value)
            .expect("serializing a scalar to YAML never fails")
            .trim_end()
            .to_string();
        return Ok(flow::upsert(text, &collection, dep, &value_text));
    }
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

/// Write a `dep: value` entry into an existing catalog mapping at the
/// position the reorder pass would choose (sorted-in when the block is
/// sorted, appended otherwise).
fn write_entry(text: &str, target: &Target, dep: &str, specifier: &str) -> String {
    write_entry_at(text, &target.path(), dep, specifier)
}

/// [`write_entry`] addressed by an explicit mapping path, so non-catalog
/// blocks (e.g. `configDependencies`) can reuse the reorder-aware splice.
fn write_entry_at(text: &str, path: &[&str], dep: &str, specifier: &str) -> String {
    write_rendered_entry_at(text, path, dep, &render::render_value(specifier))
}

/// [`write_entry_at`] for an already-rendered value text, so non-string
/// blocks (e.g. `allowBuilds`'s `true` / `false`) can reuse the
/// reorder-aware splice without going through [`render::render_value`].
///
/// A block-style mapping gains a new entry line; a single-line flow mapping
/// is rebuilt with the entry upserted, since a flow mapping the caller
/// thought was entry-less may well already hold `dep`.
fn write_rendered_entry_at(text: &str, path: &[&str], dep: &str, value_text: &str) -> String {
    if let Inline::Flow(collection) = locate_mapping(text, path) {
        return flow::upsert(text, &collection, dep, value_text);
    }
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

/// Drop `keys` from the mapping at `path`, whether it is written in block
/// or single-line flow style.
fn remove_mapping_entries(text: &str, path: &[&str], keys: &[String]) -> String {
    if let Inline::Flow(collection) = locate_mapping(text, path) {
        return flow::remove_keys(text, &collection, keys);
    }
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
    // The span runs up to the next top-level key, so it carries the blank
    // line that separates this block from that one. The last block in a
    // document has no such successor: its separator is the blank line
    // *before* it, which has to go too, or the file is left ending in a
    // blank line that the next insert would then separate from again.
    let start = if span.block_end == text.len()
        && !blanks_belong_to_kept_scalar(text, span.key_line_start)
    {
        blank_run_start(text, span.key_line_start)
    } else {
        span.key_line_start
    };
    let mut out = text.to_string();
    out.replace_range(start..span.block_end, "");
    out
}

/// Whether the blank lines that end at `line_start` are the tail of a
/// keep-chomped block scalar rather than a separator. Such a scalar keeps
/// the blank lines that follow it *as its value*, so dropping them would
/// rewrite a setting the caller never asked to touch.
///
/// The blanks belong to the scalar when the content above them climbs out to
/// a keep-chomping header: walking up, each line that is less indented than
/// everything seen so far either is that header or becomes the new bar to
/// clear. A top-level line that is not a header ends the search — a scalar's
/// body is always indented past its own key.
fn blanks_belong_to_kept_scalar(text: &str, line_start: usize) -> bool {
    let all = lines(&text[..line_start]);
    let mut enclosing_indent = usize::MAX;
    for line in all.iter().rev().filter(|line| !line.content.trim().is_empty()) {
        let indent = indent_width(line.content);
        if indent >= enclosing_indent {
            continue;
        }
        if is_kept_chomping_header(line.content) {
            return true;
        }
        if indent == 0 {
            return false;
        }
        enclosing_indent = indent;
    }
    false
}

/// Whether `content` declares a block scalar that keeps its trailing line
/// breaks. Only the value position counts — a `|+` inside an ordinary
/// scalar, inside a comment, or inside a quoted key is text.
fn is_kept_chomping_header(content: &str) -> bool {
    let mut line = content.trim_start();
    while let Some(item) = line.strip_prefix("- ") {
        line = item.trim_start();
    }
    opens_kept_chomping_scalar(line) || line_value(line).is_some_and(opens_kept_chomping_scalar)
}

/// The value of a `key: value` line, or `None` when the line declares none.
/// The delimiter is the first `:` that ends the line or is followed by
/// whitespace *outside* a quoted scalar and before any comment, so neither a
/// quoted key holding `: ` nor a comment holding one is mistaken for it.
fn line_value(line: &str) -> Option<&str> {
    let mut quote = None;
    let mut escaped = false;
    for (index, char) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if let Some(open) = quote {
            match char {
                '\\' if open == '"' => escaped = true,
                // A doubled quote inside a single-quoted scalar is one
                // escaped quote, not the end of the scalar.
                '\'' if open == '\'' && line[index + 1..].starts_with('\'') => escaped = true,
                _ if char == open => quote = None,
                _ => {}
            }
        } else {
            match char {
                // A quote only opens a scalar at the start of a token: the
                // apostrophe in a plain key like `it's` is part of the key.
                '\'' | '"'
                    if index == 0
                        || line[..index].ends_with([' ', '\t', ':', '-', '[', '{', ',']) =>
                {
                    quote = Some(char);
                }
                '#' if index == 0 || line[..index].ends_with([' ', '\t']) => return None,
                ':' => {
                    let value = &line[index + 1..];
                    if value.is_empty() || value.starts_with([' ', '\t']) {
                        return Some(value.trim_start());
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Whether `value` is a block scalar header carrying a `+`, in either order
/// relative to an explicit indentation digit (`|+`, `>+2`, `|2+`), with or
/// without a trailing comment. Any anchor and tag properties in front of the
/// header (`&notes |+`, `!!str >+`) are skipped.
fn opens_kept_chomping_scalar(value: &str) -> bool {
    let mut value = value.trim_start();
    while value.starts_with(['&', '!']) {
        let Some((_, rest)) = value.split_once([' ', '\t']) else {
            return false;
        };
        value = rest.trim_start();
    }
    let Some(indicators) = value.strip_prefix(['|', '>']) else {
        return false;
    };
    let indicators = indicators.split_whitespace().next().unwrap_or_default();
    indicators.contains('+') && indicators.chars().all(|char| char == '+' || char.is_ascii_digit())
}

/// Leading-space count of `content`, whatever the line holds — unlike
/// [`structural_indent`], which reads a comment or a blank as unindented.
/// Block scalar bodies can hold both.
fn indent_width(content: &str) -> usize {
    content.len() - content.trim_start().len()
}

/// Start of the run of blank lines immediately preceding `line_start`, or
/// `line_start` itself when the preceding line is not blank.
fn blank_run_start(text: &str, line_start: usize) -> usize {
    let mut start = line_start;
    for line in lines(&text[..line_start]).iter().rev() {
        if !line.content.trim().is_empty() {
            break;
        }
        start = line.start;
    }
    start
}

/// Write a new named catalog (`<name>:` + its first entry) into an existing
/// top-level `catalogs:` block, at the position the reorder pass would choose.
fn write_named_subblock(manifest: &Manifest, name: &str, dep: &str, value: &str) -> String {
    let text = manifest.text();
    if let Inline::Flow(collection) = locate_mapping(text, &["catalogs"]) {
        let entry = format!("{{ {}: {value} }}", render::render_value(dep));
        return flow::upsert(text, &collection, name, &entry);
    }
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
    let blank_style = manifest.blank_line_style;

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
        if blank_style && !out.is_empty() && !ends_with_blank_line(&out) {
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
    // Preserve any trailing comment, and replace the whole value.
    // Ending the value at its first whitespace would truncate a
    // multi-word plain scalar and leave the tail behind as garbage —
    // `allowBuilds` entries carry exactly such a value while they still
    // hold pnpm's `set this to true or false` placeholder.
    let after = content[colon + 1..].trim_start();
    let trailing = match comment_start(after) {
        Some(idx) => format!(" {}", &after[idx..]),
        None => String::new(),
    };
    let new_line = format!("{key_text}: {}{trailing}\n", render_bool(value));

    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..entry.line_start]);
    out.push_str(&new_line);
    out.push_str(&text[entry.line_end..]);
    out
}

/// Byte offset of a value's trailing comment, if it has one.
///
/// A `#` opens a comment only when whitespace precedes it and it sits
/// outside a quoted scalar, so neither a `#` within the value nor one in
/// a quoted string is mistaken for a comment.
///
/// A quote delimits a scalar only when it opens the value: YAML has no
/// way to start quoting partway through, so `don't` is a plain scalar
/// holding an apostrophe, not an unterminated quote.
fn comment_start(value: &str) -> Option<usize> {
    let scan_from = match value.as_bytes().first() {
        Some(&quote @ (b'"' | b'\'')) => closing_quote(value, quote)? + 1,
        _ => 0,
    };
    let bytes = value.as_bytes();
    (scan_from..bytes.len())
        .find(|&idx| bytes[idx] == b'#' && idx > 0 && bytes[idx - 1].is_ascii_whitespace())
}

/// Byte offset of the quote closing the scalar `value` opens with.
/// `None` when it is never closed, which leaves the value unparsable —
/// the caller then treats the whole of it as the value rather than
/// guessing where a comment might start.
///
/// Escaping differs by quote style: a double-quoted scalar escapes with
/// `\`, a single-quoted one by doubling the quote.
fn closing_quote(value: &str, quote: u8) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut idx = 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\\' if quote == b'"' => idx += 2,
            byte if byte == quote => {
                if quote == b'\'' && bytes.get(idx + 1) == Some(&quote) {
                    idx += 2;
                } else {
                    return Some(idx);
                }
            }
            _ => idx += 1,
        }
    }
    None
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

/// The starting offset of a top-level key's line.
fn top_level_span(text: &str, key: &str) -> Option<TopLevelSpan> {
    let all = lines(text);
    let key_idx = all.iter().position(|line| {
        structural_indent(line.content) == Some(0) && line_key(line.content).as_deref() == Some(key)
    })?;
    // A flow collection written across several lines closes at column zero,
    // which would otherwise read as the next top-level key and leave the
    // closing bracket behind when the block is replaced or removed.
    let body_start = inline_value_last_line(text, &all, key_idx).unwrap_or(key_idx) + 1;
    let next_key_idx = (body_start..all.len())
        .find(|&idx| structural_indent(all[idx].content) == Some(0))
        .unwrap_or(all.len());
    let block_end_idx = leading_comment_start(&all, body_start, next_key_idx);
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

/// Index of the line where the flow collection written inline on
/// `key_idx`'s line closes. `None` when that line carries no inline value,
/// its value is not a flow collection, or the collection never closes.
fn inline_value_last_line(text: &str, all: &[Line<'_>], key_idx: usize) -> Option<usize> {
    let open = inline_value_on_line(text, all[key_idx].start)?;
    if !text[open..].starts_with(['{', '[']) {
        return None;
    }
    let close = flow::closing_bracket_across_lines(text, open)?;
    all.iter().position(|line| line.start <= close && close < line.end)
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
/// Judged once, on the document as parsed: an edit that drops a key must not
/// change how the surviving blocks are separated.
pub(crate) fn uses_blank_line_style(text: &str, top_level_keys: &[String]) -> bool {
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

/// Whether `text`'s final line is blank, by the same trimmed-content test
/// [`blank_run_start`] and [`has_blank_before`] use — so a whitespace-only
/// separator counts as one too.
fn ends_with_blank_line(text: &str) -> bool {
    lines(text).last().is_some_and(|line| line.content.trim().is_empty())
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

    let prunable: HashSet<String> = allow_builds
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
        .collect();

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

    let new_text = match locate_mapping(manifest.text(), &[BLOCK]) {
        Inline::Flow(collection) => {
            let prunable: Vec<String> = prunable.iter().cloned().collect();
            flow::remove_keys(manifest.text(), &collection, &prunable)
        }
        Inline::Unsupported => return false,
        Inline::Block => {
            let entries = match locate(manifest.text(), &[BLOCK]) {
                Some(mapping) if !mapping.entries.is_empty() => mapping.entries,
                _ => return false,
            };
            // Entries are removed by pairing each text line with its decoded
            // key — the raw key text can differ from the decoded form
            // (quoting, escapes). A count mismatch means the two views
            // disagree (e.g. duplicate keys), so leave the block untouched.
            if entries.len() != all_keys.len() {
                return false;
            }
            let mut out = manifest.text().to_string();
            for (entry, key) in entries.iter().zip(&all_keys).rev() {
                if prunable.contains(key) {
                    out.replace_range(entry.line_start..entry.block_end, "");
                }
            }
            out
        }
    };

    if let Some(builds) = manifest.allow_builds.as_mut() {
        for key in &prunable {
            builds.shift_remove(key);
        }
    }
    manifest.set_text(new_text);
    true
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
