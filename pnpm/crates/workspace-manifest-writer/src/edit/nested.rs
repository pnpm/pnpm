//! Upserting one scalar entry at the end of a key path, creating every
//! mapping on the path that is missing.

use super::{
    Inline, Manifest, flow, insert_top_level_block, locate, locate_mapping, mapping_keys, render,
    replace_value_text_at, splice, write_rendered_entry_at,
};

/// The deepest mapping of a key path that the document has.
enum Parent {
    /// Not even the top-level key.
    Missing,
    /// The block mapping at this depth.
    Block(usize),
    /// The single-line flow mapping at this depth, which does not hold the
    /// next key of the path.
    Flow(usize, flow::Collection),
}

/// Upsert `<path>.<key>: <value>`, creating the mappings of `path` that are
/// missing. `value` is the rendered text and the JSON it reads back as,
/// compared against what the document records for the no-op decision.
/// Returns whether anything changed.
pub(super) fn set_nested_entry(
    manifest: &mut Manifest,
    path: &[&str],
    key: &str,
    (value_text, value_json): (&str, &serde_json::Value),
) -> bool {
    let text = manifest.document.text();
    if recorded_value(text, path, key).as_ref() == Some(value_json) {
        return false;
    }
    let new_text = match deepest_mapping(text, path) {
        Parent::Missing => {
            let block = render_block_chain(&path[1..], key, value_text, 2);
            let block = format!("{}:\n{block}", path[0]);
            let new_text = insert_top_level_block(manifest, path[0], &block);
            manifest.document.keys =
                render::target_order(&manifest.document.keys, &[path[0].to_string()]);
            new_text
        }
        Parent::Flow(depth, collection) => {
            let (first, value) = match path[depth..].split_first() {
                Some((first, rest)) => (*first, render_flow_chain(rest, key, value_text)),
                None => (key, value_text.to_string()),
            };
            flow::upsert(text, &collection, first, &value)
        }
        Parent::Block(depth) if depth < path.len() => {
            write_chain_at(text, &path[..depth], &path[depth..], key, value_text)
        }
        Parent::Block(_)
            if mapping_keys(text, path)
                .iter()
                .any(|existing| existing == key) =>
        {
            replace_value_text_at(text, path, key, value_text)
        }
        Parent::Block(_) => write_rendered_entry_at(text, path, key, value_text),
    };
    manifest.document.set_text(new_text);
    true
}

fn deepest_mapping(text: &str, path: &[&str]) -> Parent {
    let mut found = Parent::Missing;
    for depth in 1..=path.len() {
        let prefix = &path[..depth];
        match locate_mapping(text, prefix) {
            Inline::Flow(collection) => {
                let holds_next = holds_key(&collection, path.get(depth));
                found = Parent::Flow(depth, collection);
                if !holds_next {
                    return found;
                }
            }
            Inline::Block if locate(text, prefix).is_some() => found = Parent::Block(depth),
            Inline::Block | Inline::Unsupported => return found,
        }
    }
    found
}

fn holds_key(collection: &flow::Collection, key: Option<&&str>) -> bool {
    key.is_some_and(|key| {
        collection
            .keys()
            .iter()
            .any(|existing| existing == key)
    })
}

/// Write `chain`'s mappings, ending in `key: value`, as a new entry of the
/// existing block mapping at `parent`, at the position the reorder pass
/// would choose.
pub(super) fn write_chain_at(
    text: &str,
    parent: &[&str],
    chain: &[&str],
    key: &str,
    value: &str,
) -> String {
    let mapping = locate(text, parent).expect("parent mapping exists");
    let existing: Vec<String> = mapping.entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    let order = render::target_order(&existing, &[chain[0].to_string()]);
    let position = order
        .iter()
        .position(|name| name == chain[0])
        .expect("name is in the merged order");
    let block = render_block_chain(chain, key, value, mapping.entry_indent);
    let offset = if position == 0 {
        mapping.body_start
    } else {
        let predecessor = &order[position - 1];
        mapping.entries
            .iter()
            .find(|entry| &entry.key == predecessor)
            .expect("predecessor entry exists")
            .block_end
    };
    splice(text, offset, &block)
}

/// `chain`'s keys as nested block mappings indented from `indent`, the last
/// holding `key: value`.
fn render_block_chain(chain: &[&str], key: &str, value: &str, indent: usize) -> String {
    let leaf = format!(
        "{}{}: {value}\n",
        " ".repeat(indent + 2 * chain.len()),
        render::render_value(key),
    );
    chain
        .iter()
        .enumerate()
        .map(|(level, name)| {
            format!("{}{}:\n", " ".repeat(indent + 2 * level), render::render_value(name))
        })
        .chain(std::iter::once(leaf))
        .collect()
}

/// `chain`'s keys as nested flow mappings, the innermost holding
/// `key: value`.
fn render_flow_chain(chain: &[&str], key: &str, value: &str) -> String {
    let inner = format!("{{ {}: {value} }}", render::render_value(key));
    chain
        .iter()
        .rev()
        .fold(inner, |inner, name| format!("{{ {}: {inner} }}", render::render_value(name)))
}

fn recorded_value(text: &str, path: &[&str], key: &str) -> Option<serde_json::Value> {
    let document = serde_saphyr::from_str::<serde_json::Value>(text).ok()?;
    path.iter()
        .chain(std::iter::once(&key))
        .try_fold(&document, |value, name| value.get(name))
        .cloned()
}
