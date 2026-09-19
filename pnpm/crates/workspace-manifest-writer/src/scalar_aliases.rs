//! YAML path queries follow aliases to their definitions. Expand scalar
//! aliases before editing so changing one entry cannot change another, then
//! restore references between surviving entries whose values still agree.

mod paths;

use self::paths::{ScalarPath, scalar_paths};
use serde_saphyr::granit_parser::{
    Event, Parser, ScalarStyle, Scanner, Span, StrInput, Token, TokenType,
};
use std::{collections::HashMap, ops::Range};
use yamlpath::{Document, Route};

/// Scalar alias identities retained while edits operate on independent values.
#[derive(Default)]
pub(crate) struct ScalarAliases {
    groups: Vec<Group>,
}

struct Group {
    name: String,
    paths: Vec<ScalarPath>,
    implicit_null: bool,
}

struct Definition {
    name: String,
    anchor: Range<usize>,
    value: Range<usize>,
    tag: Option<Range<usize>>,
    aliases: Vec<Range<usize>>,
    implicit_null: bool,
    block: bool,
}

impl ScalarAliases {
    pub(crate) fn expand(text: &str) -> Result<(String, Self), Box<yamlpatch::Error>> {
        if !text.contains('&') {
            return Ok((text.to_string(), Self::default()));
        }
        let anchors = anchor_tokens(text);
        if anchors.is_empty() {
            return Ok((text.to_string(), Self::default()));
        }
        let mut names = HashMap::<String, usize>::new();
        for (name, _) in &anchors {
            *names.entry(name.clone()).or_default() += 1;
        }
        let mut definitions = scalar_definitions(text, &anchors)?;
        if definitions.is_empty() {
            return Ok((text.to_string(), Self::default()));
        }
        let normalized = normalize_implicit_nulls(text, &mut definitions)?;
        let text = normalized.as_deref().unwrap_or(text);
        expand_definitions(text, definitions, names)
    }

    pub(crate) fn restore(self, text: &str) -> Result<String, Box<yamlpatch::Error>> {
        if self.groups.is_empty() {
            return Ok(text.to_string());
        }
        let document = Document::new(text.to_string()).map_err(yamlpatch::Error::from)?;
        let mut edits = Vec::new();
        for group in self.groups {
            restore_group(&document, group, &mut edits)?;
        }
        Ok(apply_edits(text, edits))
    }
}

fn byte_range(span: Span) -> Range<usize> {
    span.start.byte_offset().expect("string parser records byte offsets")
        ..span.end.byte_offset().expect("string parser records byte offsets")
}

fn apply_edits(text: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut output = String::with_capacity(text.len());
    let mut end = 0;
    for (range, replacement) in edits {
        output.push_str(&text[end..range.start]);
        output.push_str(&replacement);
        end = range.end;
    }
    output.push_str(&text[end..]);
    output
}

fn invalid(message: String) -> yamlpatch::Error {
    yamlpatch::Error::InvalidOperation(message)
}

fn anchor_tokens(text: &str) -> Vec<(String, Range<usize>)> {
    Scanner::new(StrInput::new(text))
        .filter_map(|Token(span, token)| match token {
            TokenType::Anchor(name) => Some((name.into_owned(), byte_range(span))),
            _ => None,
        })
        .collect()
}

fn scalar_definitions(
    text: &str,
    anchors: &[(String, Range<usize>)],
) -> Result<HashMap<usize, Definition>, Box<yamlpatch::Error>> {
    let tags = tag_tokens(text);
    let mut definitions = HashMap::<usize, Definition>::new();
    let mut parser = Parser::new_from_str(text);
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| invalid(error.to_string()))?;
        match event {
            Event::Scalar(_, style, id, _) if id != 0 => {
                let (name, anchor) = anchors
                    .get(id - 1)
                    .ok_or_else(|| invalid("Missing scalar anchor token".to_string()))?;
                definitions.insert(
                    id,
                    Definition {
                        name: name.clone(),
                        anchor: anchor.clone(),
                        value: byte_range(span),
                        tag: span.tag_start
                            .and_then(|start| start.byte_offset())
                            .and_then(|start| tags.get(&start).cloned()),
                        aliases: Vec::new(),
                        implicit_null: false,
                        block: matches!(style, ScalarStyle::Literal | ScalarStyle::Folded),
                    },
                );
            }
            Event::Alias(id) => {
                if let Some(definition) = definitions.get_mut(&id) {
                    definition.aliases.push(byte_range(span));
                }
            }
            _ => {}
        }
    }
    Ok(definitions)
}

fn expand_definition(
    text: &str,
    definition: &Definition,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), Box<yamlpatch::Error>> {
    let replacement = scalar_replacement(text, definition)?;
    for alias in &definition.aliases {
        edits.push((alias.clone(), replacement.clone()));
    }
    if let Some(tag) = &definition.tag {
        let replacement = if definition.block && text[..definition.value.end].ends_with('\n') {
            format!("{replacement}\n")
        } else {
            replacement
        };
        edits.push((definition.anchor.start.min(tag.start)..definition.value.end, replacement));
        return Ok(());
    }
    let mut anchor = definition.anchor.clone();
    while text.as_bytes().get(anchor.end) == Some(&b' ') {
        anchor.end += 1;
    }
    edits.push((anchor, String::new()));
    Ok(())
}

fn unique_name(
    mut name: String,
    names: &mut HashMap<String, usize>,
    next_suffix: &mut HashMap<String, usize>,
) -> String {
    if names
        .get(&name)
        .copied()
        .unwrap_or_default()
        > 1
    {
        let suffix = next_suffix.entry(name.clone()).or_insert(1);
        while names.contains_key(&format!("{name}_{suffix}")) {
            *suffix += 1;
        }
        name = format!("{name}_{suffix}");
        *suffix += 1;
        names.insert(name.clone(), 1);
    }
    name
}

type SurvivingValue<'a> = (Range<usize>, yaml_serde::Value, &'a str, bool);

fn surviving_values(
    document: &Document,
    paths: Vec<ScalarPath>,
) -> Result<Vec<SurvivingValue<'_>>, Box<yamlpatch::Error>> {
    let mut values = Vec::new();
    for path in paths {
        let route = Route::from(path.route);
        if !document.query_exists(&route) {
            continue;
        }
        let feature = if path.key {
            Some(document.query_key_only(&route).map_err(yamlpatch::Error::from)?)
        } else {
            document.query_exact(&route).map_err(yamlpatch::Error::from)?
        };
        let Some(feature) = feature else { continue };
        let literal = document.extract(&feature);
        let value: yaml_serde::Value =
            serde_saphyr::from_str(literal).map_err(|error| invalid(error.to_string()))?;
        if value.is_mapping() || value.is_sequence() {
            continue;
        }
        let (start, end) = feature.location.byte_span;
        values.push((start..end, value, literal, path.key));
    }
    Ok(values)
}

fn restore_group(
    document: &Document,
    group: Group,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), Box<yamlpatch::Error>> {
    let mut values = surviving_values(document, group.paths)?;
    values.sort_by_key(|(span, ..)| span.start);
    values.dedup_by_key(|(span, ..)| span.start);
    let Some((_, first, ..)) = values.first() else { return Ok(()) };
    let first = first.clone();
    for (index, (span, value, literal, key)) in values.into_iter().enumerate() {
        if index == 0 {
            let replacement =
                format_anchor(&group.name, literal, group.implicit_null && value.is_null());
            edits.push((span, replacement));
        } else if value == first {
            let replacement = format_alias(&group.name, key, &document.source()[span.end..]);
            edits.push((span, replacement));
        }
    }
    Ok(())
}

fn normalize_implicit_nulls(
    text: &str,
    definitions: &mut HashMap<usize, Definition>,
) -> Result<Option<String>, Box<yamlpatch::Error>> {
    let implicit_ids: Vec<_> = definitions
        .iter()
        .filter(|(_, definition)| definition.value.is_empty() && definition.tag.is_none())
        .map(|(id, _)| *id)
        .collect();
    if implicit_ids.is_empty() {
        return Ok(None);
    }
    let edits = implicit_ids
        .iter()
        .map(|id| {
            let end = definitions[id].anchor.end;
            (end..end, " null".to_string())
        })
        .collect();
    let normalized = apply_edits(text, edits);
    *definitions = scalar_definitions(&normalized, &anchor_tokens(&normalized))?;
    for id in implicit_ids {
        definitions
            .get_mut(&id)
            .ok_or_else(|| invalid(format!("Missing scalar anchor {id} after null normalization")))?
            .implicit_null = true;
    }
    Ok(Some(normalized))
}

fn expand_definitions(
    text: &str,
    definitions: HashMap<usize, Definition>,
    mut names: HashMap<String, usize>,
) -> Result<(String, ScalarAliases), Box<yamlpatch::Error>> {
    let mut paths_by_id = scalar_paths(text)?;
    let mut edits = Vec::new();
    let mut groups = Vec::new();
    let mut definitions: Vec<_> = definitions.into_iter().collect();
    definitions.sort_by_key(|(_, definition)| definition.anchor.start);
    let mut next_suffix = HashMap::new();
    for (id, definition) in definitions {
        let Some(paths) = paths_by_id.remove(&id) else {
            continue;
        };
        expand_definition(text, &definition, &mut edits)?;
        let name = unique_name(definition.name, &mut names, &mut next_suffix);
        groups.push(Group { name, paths, implicit_null: definition.implicit_null });
    }
    Ok((apply_edits(text, edits), ScalarAliases { groups }))
}

fn scalar_replacement(
    text: &str,
    definition: &Definition,
) -> Result<String, Box<yamlpatch::Error>> {
    let literal = &text[definition.value.clone()];
    let start = definition.tag
        .as_ref()
        .map_or(definition.anchor.start, |tag| tag.start.min(definition.anchor.start));
    let value: yaml_serde::Value = serde_saphyr::from_str(&text[start..definition.value.end])
        .map_err(|error| invalid(error.to_string()))?;
    Ok(match &value {
        yaml_serde::Value::String(_)
            if definition.tag.is_none() && !definition.block && !literal.contains(['\n', '\r']) =>
        {
            literal.to_string()
        }
        yaml_serde::Value::String(value) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        _ => yaml_serde::to_string(&value)
            .map_err(yamlpatch::Error::from)?
            .trim_end()
            .to_string(),
    })
}

fn tag_tokens(text: &str) -> HashMap<usize, Range<usize>> {
    Scanner::new(StrInput::new(text))
        .filter_map(|Token(span, token)| {
            matches!(token, TokenType::Tag(..)).then(|| (byte_range(span).start, byte_range(span)))
        })
        .collect()
}

fn format_anchor(name: &str, literal: &str, implicit_null: bool) -> String {
    if implicit_null { format!("&{name}") } else { format!("&{name} {literal}") }
}

fn format_alias(name: &str, key: bool, following: &str) -> String {
    let separator = if key && following.starts_with(':') { " " } else { "" };
    format!("*{name}{separator}")
}
