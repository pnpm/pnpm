use super::{
    Component, DEFAULT_CATALOG_NAME, Document, IndexMap, Inline, Manifest, Op, Patch, Route,
    comment_start, flow, insert_top_level_block, locate, locate_mapping, render, render_bool,
    splice, structural_colon_index,
};

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
pub(super) fn upsert(
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
pub(super) fn replace_value_at(
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
pub(super) fn write_entry_at(text: &str, path: &[&str], dep: &str, specifier: &str) -> String {
    write_rendered_entry_at(text, path, dep, &render::render_value(specifier))
}

/// [`write_entry_at`] for an already-rendered value text, so non-string
/// blocks (e.g. `allowBuilds`'s `true` / `false`) can reuse the
/// reorder-aware splice without going through [`render::render_value`].
///
/// A block-style mapping gains a new entry line; a single-line flow mapping
/// is rebuilt with the entry upserted, since a flow mapping the caller
/// thought was entry-less may well already hold `dep`.
pub(super) fn write_rendered_entry_at(
    text: &str,
    path: &[&str],
    dep: &str,
    value_text: &str,
) -> String {
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

/// Rewrite the scalar value of `key`'s existing entry under `path` in place,
/// preserving the key's text/quoting and any trailing comment. Used for
/// `allowBuilds` instead of the `yamlpatch` route, which rejects a key
/// containing `:` (an artifact pkgId such as `foo@https://example.com/foo.tgz`).
pub(super) fn replace_bool_value_at(text: &str, path: &[&str], key: &str, value: bool) -> String {
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
