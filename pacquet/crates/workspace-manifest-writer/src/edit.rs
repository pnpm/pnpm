//! The catalog merge + format-preserving edit pass.
//!
//! Ports pnpm's
//! [`addCatalogs`](https://github.com/pnpm/pnpm/blob/e7e99f04e4/workspace/workspace-manifest-writer/src/index.ts#L125-L159)
//! together with the slice of `patchDocument` /
//! `propagateBlankLinesToNewPairs` it relies on. Because `updatedCatalogs`
//! only ever inserts new entries/blocks or updates a single value (existing
//! entries never move relative to each other), the format-preserving edits
//! are expressed as targeted text splices for inserts and a [`yamlpatch`]
//! `Op::Replace` for value updates.

use indexmap::IndexMap;
use pacquet_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use yamlpatch::{Op, Patch};
use yamlpath::{Component, Document, Route};

use crate::{model::Manifest, render};

/// Merge `updated` into `manifest`'s catalog blocks. Returns whether anything
/// changed (mirrors pnpm's `shouldBeUpdated`).
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
/// creating the block if absent. Returns whether anything changed. Mirrors
/// the `allowBuilds` half of pnpm's
/// [`writeSettings`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/config/writer/src/index.ts),
/// which `pnpm approve-builds` calls with each approved package set to
/// `true` and each denied/unselected package set to `false`.
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
        // shorthand, matching pnpm's `addCatalogs`.
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
/// position pnpm's reorder pass would choose (sorted-in when the block is
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
/// the position pnpm's reorder + blank-line passes would choose.
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

/// Whether every original non-first top-level key has a blank line before it
/// (pnpm's blank-line-style detection).
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
