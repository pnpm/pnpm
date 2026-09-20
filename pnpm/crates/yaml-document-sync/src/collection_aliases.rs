use super::{Error, inline, scalar_aliases::byte_range, splice};
use serde_json::Value;
use serde_saphyr::granit_parser::{Scanner, StrInput, Token, TokenType};
use std::{collections::HashMap, ops::Range};
use yamlpath::{Document, Route};

/// Detach collection references when either side changes. YAML path queries
/// otherwise edit the anchor definition even when addressing an alias.
pub(super) fn detach_changed(
    document: &mut Document,
    original: &Value,
    target: &Value,
) -> Result<(), Box<Error>> {
    let aliases: Vec<_> = Scanner::new(StrInput::new(document.source()))
        .filter_map(|Token(span, token)| {
            matches!(token, TokenType::Alias(_)).then(|| byte_range(span))
        })
        .collect();
    if aliases.is_empty() {
        return Ok(());
    }
    let mut collector =
        Collector { document, aliases, definitions: HashMap::new(), edits: Vec::new() };
    collector.visit(&Route::default(), original, Some(target))?;
    let mut edits = collector.edits;
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, text) in edits {
        splice(document, range, &text)?;
    }
    Ok(())
}

struct Collector<'doc, 'value> {
    document: &'doc Document,
    aliases: Vec<Range<usize>>,
    definitions: HashMap<usize, Option<&'value Value>>,
    edits: Vec<(Range<usize>, String)>,
}

impl<'value> Collector<'_, 'value> {
    fn visit(
        &mut self,
        route: &Route<'_>,
        original: &Value,
        target: Option<&'value Value>,
    ) -> Result<(), Box<Error>> {
        if !original.is_object() && !original.is_array() {
            return Ok(());
        }
        let exact = self.document
            .query_exact(route)
            .map_err(Error::from)?
            .ok_or_else(|| Error::InvalidOperation("Expected a YAML collection".to_string()))?;
        let pretty = self.document.query_pretty(route).map_err(Error::from)?;
        let (start, end) = pretty.location.byte_span;
        let (value_start, value_end) = exact.location.byte_span;
        if value_start < start || value_end > end {
            self.detach_alias(start..end, value_start, original, target)?;
            return Ok(());
        }
        self.definitions.insert(value_start, target);
        self.visit_children(route, original, target)
    }

    fn visit_children(
        &mut self,
        route: &Route<'_>,
        original: &Value,
        target: Option<&'value Value>,
    ) -> Result<(), Box<Error>> {
        match original {
            Value::Object(mapping) => {
                for (key, value) in mapping {
                    self.visit(
                        &route.with_key(key.as_str()),
                        value,
                        target.and_then(|target| target.get(key)),
                    )?;
                }
            }
            Value::Array(sequence) => {
                for (index, value) in sequence.iter().enumerate() {
                    self.visit(
                        &route.with_key(index),
                        value,
                        target.and_then(|target| target.get(index)),
                    )?;
                }
            }
            _ => unreachable!("only collections are visited"),
        }
        Ok(())
    }
    fn detach_alias(
        &mut self,
        span: Range<usize>,
        definition: usize,
        original: &Value,
        target: Option<&Value>,
    ) -> Result<(), Box<Error>> {
        if target == Some(original)
            && self.definitions
                .get(&definition)
                .copied()
                .flatten()
                == Some(original)
        {
            return Ok(());
        }
        let range = self.aliases
            .iter()
            .find(|range| range.start >= span.start && range.end <= span.end)
            .ok_or_else(|| {
                Error::InvalidOperation("Cannot locate the collection alias".to_string())
            })?;
        self.edits.push((range.clone(), inline(original)?));
        Ok(())
    }
}
