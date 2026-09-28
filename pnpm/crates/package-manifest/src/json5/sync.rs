use super::stringify::{is_identifier, quote_string};
use jsonc_parser::{
    ParseOptions,
    cst::{
        CstArray, CstContainerNode, CstInputValue, CstLeafNode, CstNode, CstObject, CstObjectProp,
        CstRootNode, CstStringLit,
    },
};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// Edit `source`, which decodes to `original`, so it decodes to `target`.
/// Only the values that differ are rewritten, so comments, quotes, and
/// layout elsewhere stay as written. New keys are placed after the key that
/// precedes them in `target`, and new keys and strings follow the quoting of
/// `source`. Returns `None` when the CST cannot parse `source`.
pub(crate) fn sync(source: &str, original: &Value, target: &Value) -> Option<String> {
    let root = CstRootNode::parse(source, &ParseOptions::default()).ok()?;
    let mut editor = Editor { style: Style::detect(&root), bare_names: Vec::new() };
    editor.sync_node(root.value()?, original, target);
    // The CST prints a name's raw text as stored, so a bare identifier can
    // stand in the string literal it wrote. Decoding such a name panics, so
    // this runs only once no lookup by name is left.
    for name in editor.bare_names {
        if let Ok(key) = name.decoded_value() {
            name.set_raw_value(key);
        }
    }
    Some(root.to_string())
}

struct Editor {
    style: Style,
    bare_names: Vec<CstStringLit>,
}

impl Editor {
    fn sync_node(&mut self, node: CstNode, original: &Value, target: &Value) {
        if original == target {
            return;
        }
        match (original, target, node.as_object(), node.as_array()) {
            (Value::Object(original), Value::Object(target), Some(object), _) => {
                self.sync_object(&object, original, target);
            }
            (Value::Array(original), Value::Array(target), _, Some(array)) => {
                self.sync_array(&array, original, target);
            }
            (_, Value::String(text), _, _) if let Some(literal) = node.as_string_lit() => {
                requote_in_place(&literal, text);
            }
            _ => {
                if let Some(node) = replace(node, input_value(target)) {
                    self.restyle(&node);
                }
            }
        }
    }

    fn sync_array(&mut self, array: &CstArray, original: &[Value], target: &[Value]) {
        for (element, (old, new)) in array
            .elements()
            .into_iter()
            .zip(original.iter().zip(target))
        {
            self.sync_node(element, old, new);
        }
        for element in array
            .elements()
            .into_iter()
            .skip(target.len())
            .rev()
        {
            element.remove();
        }
        for value in target.iter().skip(original.len()) {
            self.restyle(&array.append(input_value(value)));
        }
    }

    fn sync_object(
        &mut self,
        object: &CstObject,
        original: &Map<String, Value>,
        target: &Map<String, Value>,
    ) {
        let properties = properties_by_name(object);
        for (key, old) in original {
            let Some(property) = properties.get(key) else { continue };
            match (target.get(key), property.value()) {
                (Some(new), Some(value)) => self.sync_node(value, old, new),
                (Some(_), None) => {}
                (None, _) => property.clone().remove(),
            }
        }
        let mut previous: Option<CstObjectProp> = None;
        for (key, value) in target {
            if let Some(property) = properties.get(key) {
                previous = Some(property.clone());
                continue;
            }
            let index = previous
                .as_ref()
                .map_or(0, |property| property.property_index() + 1);
            let property = object.insert(index, key, input_value(value));
            self.restyle(&property.clone().into());
            previous = Some(property);
        }
    }

    /// Requote the keys and strings of a node the CST wrote in double quotes.
    fn restyle(&mut self, node: &CstNode) {
        let mut pending = vec![node.clone()];
        while let Some(node) = pending.pop() {
            if let Some(property) = node.as_object_prop() {
                if let Some(name) = property.name().and_then(|name| name.as_string_lit()) {
                    self.restyle_name(name);
                }
                pending.extend(property.value());
            } else if let Some(literal) = node.as_string_lit() {
                self.style.requote(&literal);
            } else {
                pending.extend(node.children());
            }
        }
    }

    fn restyle_name(&mut self, name: CstStringLit) {
        self.style.requote(&name);
        let is_bare =
            self.style.bare_keys && name.decoded_value().is_ok_and(|key| is_identifier(&key));
        if is_bare {
            self.bare_names.push(name);
        }
    }
}

/// The quoting of a file: strings take the quote most strings in it use,
/// single on a tie, and identifier keys stay bare when any key is bare.
struct Style {
    quote: char,
    bare_keys: bool,
}

impl Style {
    fn detect(root: &CstRootNode) -> Self {
        let (mut single_quoted, mut double_quoted, mut bare_keys) = (0usize, 0usize, false);
        let mut pending = root.children();
        while let Some(node) = pending.pop() {
            match node
                .as_string_lit()
                .map(|literal| literal.raw_value().starts_with('"'))
            {
                Some(true) => double_quoted += 1,
                Some(false) => single_quoted += 1,
                None => {}
            }
            bare_keys |= node
                .as_object_prop()
                .and_then(|property| property.name())
                .is_some_and(|name| name.as_word_lit().is_some());
            pending.extend(node.children());
        }
        let quote = if double_quoted > single_quoted { '"' } else { '\'' };
        Style { quote, bare_keys }
    }

    fn requote(&self, literal: &CstStringLit) {
        if let Ok(text) = literal.decoded_value() {
            literal.set_raw_value(quote_string(&text, self.quote));
        }
    }
}

/// Write `text` into `literal` with the quote it already uses.
fn requote_in_place(literal: &CstStringLit, text: &str) {
    let quote = literal
        .raw_value()
        .chars()
        .next()
        .unwrap_or('\'');
    literal.set_raw_value(quote_string(text, quote));
}

/// The properties of `object` by decoded name, looked up once so a save
/// does not rescan the object for every key. The first of duplicate names
/// wins, as with [`CstObject::get`].
fn properties_by_name(object: &CstObject) -> HashMap<String, CstObjectProp> {
    let mut properties = HashMap::new();
    for property in object.properties() {
        if let Some(name) = property.decoded_name() {
            properties.entry(name).or_insert(property);
        }
    }
    properties
}

fn replace(node: CstNode, value: CstInputValue) -> Option<CstNode> {
    match node {
        CstNode::Container(CstContainerNode::Object(object)) => object.replace_with(value),
        CstNode::Container(CstContainerNode::Array(array)) => array.replace_with(value),
        CstNode::Leaf(CstLeafNode::StringLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::NumberLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::BooleanLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::NullKeyword(keyword)) => keyword.replace_with(value),
        CstNode::Leaf(CstLeafNode::WordLit(word)) => word.replace_with(value),
        CstNode::Container(_) | CstNode::Leaf(_) => None,
    }
}

fn input_value(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(text) => CstInputValue::String(text.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(input_value).collect()),
        Value::Object(fields) => CstInputValue::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), input_value(value)))
                .collect(),
        ),
    }
}
