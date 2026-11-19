use super::QueryError;
use serde_saphyr::granit_parser::{Event, Parser, ScalarStyle, Tag};
use std::collections::HashMap;

pub(super) const MAX_MERGE_DEPTH: usize = 8;
pub(super) type NodeId = usize;

pub(super) enum YamlNode {
    Mapping(Vec<(Option<NodeId>, NodeId)>),
    Sequence(Vec<NodeId>),
    Scalar(Option<String>),
    Alias(usize),
}

#[derive(Default)]
pub(super) struct YamlGraph {
    nodes: Vec<YamlNode>,
    anchors: HashMap<usize, NodeId>,
    pub(super) root: Option<NodeId>,
}

enum OpenContainer {
    Mapping {
        anchor_id: usize,
        entries: Vec<(Option<NodeId>, NodeId)>,
        pending_key: Option<NodeId>,
        awaiting_value: bool,
    },
    Sequence {
        anchor_id: usize,
        items: Vec<NodeId>,
    },
}

impl OpenContainer {
    fn new_mapping(anchor_id: usize) -> Self {
        Self::Mapping { anchor_id, entries: Vec::new(), pending_key: None, awaiting_value: false }
    }
}

impl YamlGraph {
    pub(super) fn parse(text: &str) -> Result<Self, QueryError> {
        let mut graph = Self::default();
        let mut stack = Vec::new();
        for next in Parser::new_from_str(text) {
            let (event, _) = next.map_err(|error| {
                let mark = error.marker();
                QueryError::InvalidInput(mark.line(), mark.col() + 1)
            })?;
            if graph.handle_event(event, &mut stack)? {
                break;
            }
        }
        Ok(graph)
    }

    fn handle_event(
        &mut self,
        event: Event<'_>,
        stack: &mut Vec<OpenContainer>,
    ) -> Result<bool, QueryError> {
        match event {
            Event::DocumentEnd if self.root.is_some() => return Ok(true),
            Event::Scalar(value, style, anchor_id, tag) => {
                let text =
                    is_string_scalar(&value, style, tag.as_deref()).then(|| value.into_owned());
                self.push_and_attach(YamlNode::Scalar(text), anchor_id, stack);
            }
            Event::Alias(anchor_id) => {
                self.push_and_attach(YamlNode::Alias(anchor_id), 0, stack);
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
                YamlNode::Alias(anchor_id) => node_id = *self.anchors.get(anchor_id)?,
                node => return Some((node_id, node)),
            }
        }
        match self.nodes.get(node_id)? {
            YamlNode::Alias(_) => None,
            node => Some((node_id, node)),
        }
    }

    pub(super) fn string_at(&self, node_id: NodeId) -> Option<&str> {
        match self.deref(node_id)?.1 {
            YamlNode::Scalar(Some(value)) => Some(value.as_str()),
            _ => None,
        }
    }
}

fn attach_node(stack: &mut [OpenContainer], root: &mut Option<NodeId>, node_id: NodeId) {
    match stack.last_mut() {
        Some(OpenContainer::Sequence { items, .. }) => items.push(node_id),
        Some(OpenContainer::Mapping {
            entries,
            pending_key,
            awaiting_value,
            ..
        }) => {
            if *awaiting_value {
                entries.push((pending_key.take(), node_id));
                *awaiting_value = false;
            } else {
                *pending_key = Some(node_id);
                *awaiting_value = true;
            }
        }
        None if root.is_none() => *root = Some(node_id),
        None => {}
    }
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
