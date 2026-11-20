use super::{QueryError, Range};
use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle, Span, Tag};
use std::collections::HashMap;

pub(super) const MAX_MERGE_DEPTH: usize = 64;
const YAML_CORE_TAG_PREFIX: &str = "tag:yaml.org,2002:";

pub(super) type NodeId = usize;

pub(super) struct StringScalar {
    pub(super) decoded: String,
    pub(super) inline_span: Option<Range<usize>>,
}

pub(super) enum KeyKind<'graph> {
    Merge,
    String(&'graph str),
}

pub(super) enum YamlNode {
    Mapping(Vec<(NodeId, NodeId)>),
    Sequence(Vec<NodeId>),
    MergeKey,
    StringScalar(StringScalar),
    OtherScalar,
    Alias { anchor_id: usize },
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
                self.push_and_attach(YamlNode::Alias { anchor_id }, 0, stack);
            }
            Event::UnresolvedAlias(_) => {
                return Err(QueryError::InvalidInput(span.start.line(), span.start.col() + 1));
            }
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
                YamlNode::Alias { anchor_id } => node_id = *self.anchors.get(anchor_id)?,
                node => return Some((node_id, node)),
            }
        }
        match self.nodes.get(node_id)? {
            YamlNode::Alias { .. } => None,
            node => Some((node_id, node)),
        }
    }

    pub(super) fn inline_string_span_at(&self, node_id: NodeId) -> Option<Range<usize>> {
        match self.deref(node_id)?.1 {
            YamlNode::StringScalar(scalar) => scalar.inline_span.clone(),
            _ => None,
        }
    }

    pub(super) fn key_at(&self, key_id: NodeId) -> Option<KeyKind<'_>> {
        match self.deref(key_id)?.1 {
            YamlNode::MergeKey => Some(KeyKind::Merge),
            YamlNode::StringScalar(scalar) => Some(KeyKind::String(scalar.decoded.as_str())),
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
        return YamlNode::MergeKey;
    }
    if !is_string_scalar(value, style, tag) {
        return YamlNode::OtherScalar;
    }
    YamlNode::StringScalar(StringScalar {
        decoded: value.to_owned(),
        inline_span: extract_inline_span(text, span, style),
    })
}

fn extract_inline_span(text: &str, span: Span, style: ScalarStyle) -> Option<Range<usize>> {
    if !matches!(style, ScalarStyle::Plain | ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted)
    {
        return None;
    }
    let range = span
        .byte_range()
        .filter(|range| !range.is_empty())?;
    let raw = text.get(range.clone())?;
    (!raw.contains('\n')).then_some(range)
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
