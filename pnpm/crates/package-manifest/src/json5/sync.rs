use super::stringify::quote_string;
use jsonc_parser::{
    ParseOptions,
    cst::{CstContainerNode, CstInputValue, CstLeafNode, CstNode, CstObject, CstRootNode},
};
use serde_json::{Map, Value};

#[cfg(test)]
mod tests;

/// Edit `source`, which decodes to `original`, so it decodes to `target`.
/// Only the values that differ are rewritten, so comments, quotes, and
/// layout elsewhere stay as written. New keys are placed after the key that
/// precedes them in `target`. Returns `None` when the CST cannot parse `source`.
pub(crate) fn sync(source: &str, original: &Value, target: &Value) -> Option<String> {
    let root = CstRootNode::parse(source, &ParseOptions::default()).ok()?;
    sync_node(root.value()?, original, target);
    Some(root.to_string())
}

fn sync_node(node: CstNode, original: &Value, target: &Value) {
    if original == target {
        return;
    }
    match (original, target, node.as_object(), node.as_array()) {
        (Value::Object(original), Value::Object(target), Some(object), _)
            if !original.is_empty() && !target.is_empty() =>
        {
            sync_object(&object, original, target);
        }
        (Value::Array(original), Value::Array(target), _, Some(array))
            if !original.is_empty() && !target.is_empty() =>
        {
            for (element, (old, new)) in array
                .elements()
                .into_iter()
                .zip(original.iter().zip(target))
            {
                sync_node(element, old, new);
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
                array.append(input_value(value));
            }
        }
        (_, Value::String(text), _, _) if let Some(literal) = node.as_string_lit() => {
            let quote = literal
                .raw_value()
                .chars()
                .next()
                .unwrap_or('\'');
            literal.set_raw_value(quote_string(text, quote));
        }
        _ => replace(node, input_value(target)),
    }
}

fn sync_object(object: &CstObject, original: &Map<String, Value>, target: &Map<String, Value>) {
    for (key, old) in original {
        let Some(property) = object.get(key) else { continue };
        match (target.get(key), property.value()) {
            (Some(new), Some(value)) => sync_node(value, old, new),
            (Some(new), None) => property.set_value(input_value(new)),
            (None, _) => property.remove(),
        }
    }
    let mut index = 0;
    for (key, value) in target {
        if let Some(property) = object.get(key) {
            index = property.property_index();
        } else {
            object.insert(index, key, input_value(value));
        }
        index += 1;
    }
}

fn replace(node: CstNode, value: CstInputValue) {
    match node {
        CstNode::Container(CstContainerNode::Object(object)) => object.replace_with(value),
        CstNode::Container(CstContainerNode::Array(array)) => array.replace_with(value),
        CstNode::Leaf(CstLeafNode::StringLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::NumberLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::BooleanLit(literal)) => literal.replace_with(value),
        CstNode::Leaf(CstLeafNode::NullKeyword(keyword)) => keyword.replace_with(value),
        CstNode::Leaf(CstLeafNode::WordLit(word)) => word.replace_with(value),
        CstNode::Container(_) | CstNode::Leaf(_) => None,
    };
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
