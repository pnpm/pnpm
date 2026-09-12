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

pub(crate) use allow_builds::{add_allow_build, add_undecided_allow_build, prune_allow_builds};
pub(crate) use catalogs::{CatalogReferences, add_catalogs, remove_unused_catalogs};
pub(crate) use policies::{
    prune_minimum_release_age_excludes, prune_trust_policy_excludes, set_audit_ignore_ghsas,
    set_minimum_release_age_excludes,
};
pub(crate) use scanning::{Inline, document_root_is_inline, has_unsupported_inline_value};
pub(crate) use spacing::uses_blank_line_style;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
};

use indexmap::IndexMap;
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use yamlpatch::{Op, Patch};
use yamlpath::{Component, Document, Route};

use crate::{
    flow,
    model::{AllowBuildValue, Manifest},
    render,
};

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

    if patched_dependencies.is_empty() {
        return Ok(drop_patched_dependencies(manifest, BLOCK));
    }

    let mut changed = drop_omitted_patches(manifest, BLOCK, patched_dependencies);
    for (key, path) in patched_dependencies {
        let current_matches = manifest
            .patched_dependencies
            .as_ref()
            .and_then(|deps| deps.get(key))
            .map(String::as_str)
            == Some(path);
        if upsert_top_level_entry(manifest, BLOCK, key, path, current_matches)? {
            manifest
                .patched_dependencies
                .get_or_insert_with(IndexMap::new)
                .insert(key.clone(), path.clone());
            changed = true;
        }
    }
    Ok(changed)
}

/// Drop the whole block, for a patch set that is now empty.
fn drop_patched_dependencies(manifest: &mut Manifest, block: &str) -> bool {
    let has_block = manifest.top_level_keys.iter().any(|key| key == block);
    if manifest.patched_dependencies.is_none() && !has_block {
        return false;
    }
    manifest.set_text(remove_top_level_block(manifest.text(), block));
    manifest.patched_dependencies = None;
    manifest.top_level_keys.retain(|key| key != block);
    true
}

/// Drop the recorded patches the new set no longer names.
fn drop_omitted_patches(
    manifest: &mut Manifest,
    block: &str,
    patched_dependencies: &IndexMap<String, String>,
) -> bool {
    let Some(existing) = manifest.patched_dependencies.as_ref() else {
        return false;
    };
    let omitted: Vec<String> =
        existing.keys().filter(|key| !patched_dependencies.contains_key(*key)).cloned().collect();
    if omitted.is_empty() {
        return false;
    }
    manifest.set_text(remove_mapping_entries(manifest.text(), &[block], &omitted));
    let current = manifest
        .patched_dependencies
        .as_mut()
        .expect("existing patched dependencies should remain decoded");
    for key in &omitted {
        current.shift_remove(key);
    }
    true
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

/// Preserve a trailing blank line before the next block, since the
/// span includes it but the freshly rendered block does not.
fn replace_top_level_block(text: &str, span: &TopLevelSpan, rendered: &str) -> String {
    let had_trailing_blank = text[span.key_line_start..span.block_end].ends_with("\n\n");
    let mut out = text.to_string();
    if had_trailing_blank {
        out.replace_range(span.key_line_start..span.block_end, &format!("{rendered}\n"));
    } else {
        out.replace_range(span.key_line_start..span.block_end, rendered);
    }
    out
}

/// Where a new `key` entry goes in `mapping` so the keys keep their target
/// order: right after its predecessor, or at the body start when first.
fn insertion_offset(mapping: &Mapping, key: &str) -> usize {
    let existing: Vec<String> = mapping.entries.iter().map(|entry| entry.key.clone()).collect();
    let order = render::target_order(&existing, &[key.to_string()]);
    let position =
        order.iter().position(|order_key| order_key == key).expect("key is in the order");
    if position == 0 {
        return mapping.body_start;
    }
    let predecessor = &order[position - 1];
    mapping
        .entries
        .iter()
        .find(|entry| &entry.key == predecessor)
        .expect("predecessor entry exists")
        .block_end
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

    let Some(successor_key) = order.get(position + 1) else {
        return append_top_level_block(text, block_text, blank_style);
    };
    let span = top_level_span(text, successor_key).expect("successor block exists");
    // Insert before the successor's key line; its existing preceding
    // blank line (if any) becomes the blank before the new block, and
    // a trailing blank line is added when the document uses that style.
    let trailing = if blank_style { "\n" } else { "" };
    splice(text, span.key_line_start, &format!("{block_text}{trailing}"))
}

/// The new key sorts last: append its block at the end of the document.
fn append_top_level_block(text: &str, block_text: &str, blank_style: bool) -> String {
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

fn splice(text: &str, offset: usize, insertion: &str) -> String {
    let mut out = String::with_capacity(text.len() + insertion.len());
    out.push_str(&text[..offset]);
    out.push_str(insertion);
    out.push_str(&text[offset..]);
    out
}

mod catalogs;

mod policies;

mod sequences;
use sequences::{reconcile_sequence_items, render_top_level_sequence, upsert_sequence_entry};

mod allow_builds;
use allow_builds::render_bool;

mod mapping;
use mapping::{
    replace_bool_value_at, replace_value_at, upsert, write_entry_at, write_rendered_entry_at,
};

mod scanning;

use scanning::{
    Line, Mapping, TopLevelSpan, comment_start, leading_comment_start, lines, locate,
    locate_mapping, locate_sequence, mapping_keys, structural_colon_index, structural_indent,
    top_level_key_line, top_level_span,
};

mod spacing;

use spacing::{blank_run_start, blanks_belong_to_kept_scalar, ends_with_blank_line};
