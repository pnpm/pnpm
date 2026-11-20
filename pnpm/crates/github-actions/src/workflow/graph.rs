use super::QueryError;
use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle, Span, Tag};
use std::collections::HashMap;

pub(super) const MAX_MERGE_DEPTH: usize = 64;
const YAML_CORE_TAG_PREFIX: &str = "tag:yaml.org,2002:";

pub(super) type NodeId = usize;

pub(super) struct StringScalar {
    pub(super) decoded: String,
    pub(super) route_key: String,
}

pub(super) enum KeyKind<'graph> {
    Merge { route_key: &'graph str },
    String { decoded: &'graph str, route_key: &'graph str },
}

pub(super) enum YamlNode {
    Mapping(Vec<(NodeId, NodeId)>),
    Sequence(Vec<NodeId>),
    MergeKey { route_key: String },
    StringScalar(StringScalar),
    OtherScalar,
    Alias { anchor_id: usize, raw_text: String },
}

#[derive(Default)]
pub(super) struct YamlGraph {
    nodes: Vec<YamlNode>,
    anchors: HashMap<usize, NodeId>,
    pub(super) root: Option<NodeId>,
}

enum OpenContainer {
    Mapping { anchor_id: usize, entries: Vec<(NodeId, NodeId)>, pending_key: Option<NodeId> },
    Sequence { anchor_id: usize, items: Vec<NodeId> },
}

impl OpenContainer {
    fn new_mapping(anchor_id: usize) -> Self {
        Self::Mapping { anchor_id, entries: Vec::new(), pending_key: None }
    }
}

impl YamlGraph {
    pub(super) fn parse(text: &str) -> Result<Self, QueryError> {
        let mut graph = Self::default();
        let mut stack = Vec::new();
        for next in Parser::new_from_str(text) {
            let (event, span) = next.map_err(|error| {
                let mark = error.marker();
                QueryError::InvalidInput(mark.line(), mark.col() + 1)
            })?;
            if graph.handle_event(text, event, span, &mut stack)? {
                break;
            }
        }
        Ok(graph)
    }

    fn handle_event(
        &mut self,
        text: &str,
        event: Event<'_>,
        span: Span,
        stack: &mut Vec<OpenContainer>,
    ) -> Result<bool, QueryError> {
        match event {
            Event::DocumentEnd if self.root.is_some() => return Ok(true),
            Event::Scalar(value, style, anchor_id, tag) => {
                let node = classify_scalar(text, &value, style, span, tag.as_deref());
                self.push_and_attach(node, anchor_id, stack);
            }
            Event::Alias(anchor_id) => {
                let raw_text = slice_span(text, span).unwrap_or_default().to_owned();
                self.push_and_attach(YamlNode::Alias { anchor_id, raw_text }, 0, stack);
            }
            Event::UnresolvedAlias(_) => return Err(QueryError::InvalidInput(0, 0)),
            Event::MappingStart(_, anchor_id, _) => {
                stack.push(OpenContainer::new_mapping(anchor_id));
            }
            Event::MappingEnd => self.finish_mapping(stack)?,
            Event::SequenceStart(_, anchor_id, _) => {
                stack.push(OpenContainer::Sequence { anchor_id, items: Vec::new() });
            }
            Event::SequenceEnd => self.finish_sequence(stack)?,
            _ => {}
        }
        Ok(false)
    }

    fn finish_mapping(&mut self, stack: &mut Vec<OpenContainer>) -> Result<(), QueryError> {
        let Some(OpenContainer::Mapping { anchor_id, entries, .. }) = stack.pop() else {
            return Err(QueryError::InvalidInput(0, 0));
        };
        self.push_and_attach(YamlNode::Mapping(entries), anchor_id, stack);
        Ok(())
    }

    fn finish_sequence(&mut self, stack: &mut Vec<OpenContainer>) -> Result<(), QueryError> {
        let Some(OpenContainer::Sequence { anchor_id, items }) = stack.pop() else {
            return Err(QueryError::InvalidInput(0, 0));
        };
        self.push_and_attach(YamlNode::Sequence(items), anchor_id, stack);
        Ok(())
    }

    fn push_and_attach(&mut self, node: YamlNode, anchor_id: usize, stack: &mut [OpenContainer]) {
        let node_id = self.nodes.len();
        self.nodes.push(node);
        if anchor_id != 0 {
            self.anchors.insert(anchor_id, node_id);
        }
        attach_node(stack, &mut self.root, node_id);
    }

    pub(super) fn deref(&self, mut node_id: NodeId) -> Option<(NodeId, &YamlNode)> {
        for _ in 0..MAX_MERGE_DEPTH {
            match self.nodes.get(node_id)? {
                YamlNode::Alias { anchor_id, .. } => node_id = *self.anchors.get(anchor_id)?,
                node => return Some((node_id, node)),
            }
        }
        match self.nodes.get(node_id)? {
            YamlNode::Alias { .. } => None,
            node => Some((node_id, node)),
        }
    }

    pub(super) fn string_at(&self, node_id: NodeId) -> Option<&str> {
        match self.deref(node_id)?.1 {
            YamlNode::StringScalar(scalar) => Some(scalar.decoded.as_str()),
            _ => None,
        }
    }

    pub(super) fn key_at(&self, key_id: NodeId) -> Option<KeyKind<'_>> {
        let alias_route = match self.nodes.get(key_id)? {
            YamlNode::Alias { raw_text, .. } if !raw_text.is_empty() => Some(raw_text.as_str()),
            _ => None,
        };
        match self.deref(key_id)?.1 {
            YamlNode::MergeKey { route_key } => {
                Some(KeyKind::Merge { route_key: alias_route.unwrap_or(route_key.as_str()) })
            }
            YamlNode::StringScalar(scalar) => Some(KeyKind::String {
                decoded: scalar.decoded.as_str(),
                route_key: alias_route.unwrap_or(scalar.route_key.as_str()),
            }),
            _ => None,
        }
    }
}

fn attach_node(stack: &mut [OpenContainer], root: &mut Option<NodeId>, node_id: NodeId) {
    match stack.last_mut() {
        Some(OpenContainer::Sequence { items, .. }) => items.push(node_id),
        Some(OpenContainer::Mapping { entries, pending_key, .. }) => match pending_key.take() {
            Some(key_id) => entries.push((key_id, node_id)),
            None => *pending_key = Some(node_id),
        },
        None if root.is_none() => *root = Some(node_id),
        None => {}
    }
}

fn classify_scalar(
    text: &str,
    value: &str,
    style: ScalarStyle,
    span: Span,
    tag: Option<&Tag>,
) -> YamlNode {
    if is_merge_scalar(value, style, tag) {
        let route_key = extract_merge_route_key(text, span);
        return YamlNode::MergeKey { route_key };
    }
    if !is_string_scalar(value, style, tag) {
        return YamlNode::OtherScalar;
    }
    let decoded = value.to_owned();
    let route_key = extract_string_route_key(text, span, style)
        .map(str::to_owned)
        .unwrap_or_else(|| decoded.clone());
    YamlNode::StringScalar(StringScalar { decoded, route_key })
}

fn is_merge_scalar(value: &str, style: ScalarStyle, tag: Option<&Tag>) -> bool {
    match tag {
        Some(tag) => is_merge_tag(tag),
        None => style == ScalarStyle::Plain && value == "<<",
    }
}

fn is_merge_tag(tag: &Tag) -> bool {
    if let Some(remaining) = YAML_CORE_TAG_PREFIX.strip_prefix(tag.handle()) {
        return tag.suffix().strip_prefix(remaining) == Some("merge");
    }
    tag.handle()
        .strip_prefix(YAML_CORE_TAG_PREFIX)
        .and_then(|tail| "merge".strip_prefix(tail))
        .is_some_and(|expected| tag.suffix() == expected)
}

fn extract_merge_route_key(text: &str, span: Span) -> String {
    if let Some(tag_token) = extract_tag_token(text, span) {
        return tag_token.to_owned();
    }
    slice_span(text, span).unwrap_or("<<").to_owned()
}

fn extract_string_route_key(text: &str, span: Span, style: ScalarStyle) -> Option<&str> {
    if let Some(tag_token) = extract_tag_token(text, span) {
        return Some(tag_token);
    }
    let raw = slice_span(text, span)?;
    match style {
        ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => {
            let mut chars = raw.chars();
            chars.next()?;
            chars.next_back()?;
            Some(chars.as_str())
        }
        ScalarStyle::Plain => Some(raw),
        _ => None,
    }
}

fn extract_tag_token(text: &str, span: Span) -> Option<&str> {
    let tag_offset = span.tag_start.and_then(|mark| mark.byte_offset())?;
    let rest = text.get(tag_offset..)?;
    if rest.starts_with("!<") {
        let close = rest.find('>')?;
        return rest.get(..=close);
    }
    let end = rest
        .char_indices()
        .find_map(|(idx, ch)| is_tag_terminator(rest, idx, ch).then_some(idx))
        .unwrap_or(rest.len());
    (!rest[..end].is_empty()).then_some(&rest[..end])
}

fn is_tag_terminator(rest: &str, idx: usize, ch: char) -> bool {
    ch.is_whitespace()
        || matches!(ch, ',' | '[' | ']' | '{' | '}')
        || (ch == ':'
            && rest[idx + 1..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace))
}

fn slice_span(text: &str, span: Span) -> Option<&str> {
    let start = span.start.byte_offset()?;
    let end = span.end.byte_offset()?;
    text.get(start..end)
}

fn is_string_scalar(value: &str, style: ScalarStyle, tag: Option<&Tag>) -> bool {
    match tag {
        Some(tag) => tag.core_suffix() == Some("str"),
        None if style != ScalarStyle::Plain => true,
        None => !matches!(
            value,
            "" | "~"
                | "null"
                | "Null"
                | "NULL"
                | "true"
                | "True"
                | "TRUE"
                | "false"
                | "False"
                | "FALSE",
        ),
    }
}
