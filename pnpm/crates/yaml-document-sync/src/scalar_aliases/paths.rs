use super::{
    byte_range,
    invalid,
};
use serde_saphyr::granit_parser::{
    Event,
    Parser,
    Span,
    StrInput,
};
use std::collections::{
    HashMap,
    HashSet,
};
use yamlpath::Component;

pub(super) struct ScalarPath {
    pub(super) route: Vec<Component<'static>>,
    pub(super) key: bool,
}

enum Container {
    Mapping { key: Option<String> },
    Sequence { index: usize },
}

#[derive(Default)]
struct PathCollector {
    containers: Vec<Container>,
    path: Vec<Component<'static>>,
    paths: HashMap<usize, Vec<ScalarPath>>,
    scalar_keys: HashMap<usize, String>,
    mapping_paths: Vec<Vec<Component<'static>>>,
    collect_mapping_paths: bool,
}

pub(super) fn scalar_paths(
    text: &str,
) -> Result<HashMap<usize, Vec<ScalarPath>>, Box<yamlpatch::Error>> {
    Ok(collect_paths(text, false)?.paths)
}

pub(crate) fn mapping_paths(
    text: &str,
) -> Result<Vec<Vec<Component<'static>>>, Box<yamlpatch::Error>> {
    Ok(collect_paths(text, true)?.mapping_paths)
}

fn collect_paths(
    text: &str,
    collect_mapping_paths: bool,
) -> Result<PathCollector, Box<yamlpatch::Error>> {
    let mut parser = Parser::new_from_str(text);
    let mut collector = PathCollector { collect_mapping_paths, ..Default::default() };
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| invalid(error.to_string()))?;
        if matches!(collector.containers.last(), Some(Container::Mapping { key: None }))
            && event.is_node()
        {
            collector.visit_key(text, &mut parser, &event, span)?;
            continue;
        }
        collector.visit(event);
    }
    Ok(collector)
}

impl PathCollector {
    fn record_scalar(&mut self, event: &Event<'_>, key: bool) {
        let id = match event {
            Event::Scalar(value, _, id, _) if *id != 0 => {
                self.scalar_keys.insert(*id, value.to_string());
                *id
            }
            Event::Alias(id) if self.scalar_keys.contains_key(id) => *id,
            _ => return,
        };
        self.paths
            .entry(id)
            .or_default()
            .push(ScalarPath { route: self.path.clone(), key });
    }

    fn visit_key<'a>(
        &mut self,
        text: &str,
        parser: &mut Parser<'a, StrInput<'a>>,
        event: &Event<'a>,
        span: Span,
    ) -> Result<(), Box<yamlpatch::Error>> {
        let key_text = match event {
            Event::Alias(id) if self.scalar_keys.contains_key(id) => self.scalar_keys[id].clone(),
            _ => mapping_key(text, parser, event, span)?,
        };
        self.path.push(Component::from(key_text.clone()));
        if self.collect_mapping_paths {
            self.mapping_paths.push(self.path.clone());
        }
        self.record_scalar(event, true);
        self.path.pop();
        if let Some(Container::Mapping { key }) = self.containers.last_mut() {
            *key = Some(key_text);
        }
        Ok(())
    }

    fn visit(&mut self, event: Event<'_>) {
        match event {
            event if event.is_node() => {
                push_component(&mut self.containers, &mut self.path);
                match event {
                    Event::MappingStart(..) => {
                        self.containers.push(Container::Mapping { key: None });
                    }
                    Event::SequenceStart(..) => {
                        self.containers.push(Container::Sequence { index: 0 });
                    }
                    _ => {
                        self.record_scalar(&event, false);
                        self.path.pop();
                    }
                }
            }
            Event::MappingEnd | Event::SequenceEnd => {
                self.containers.pop();
                self.path.pop();
            }
            _ => {}
        }
    }
}

fn push_component(containers: &mut [Container], path: &mut Vec<Component<'static>>) {
    match containers.last_mut() {
        Some(Container::Mapping { key }) => {
            path.push(Component::from(key.take().expect("mapping key precedes its value")));
        }
        Some(Container::Sequence { index }) => {
            path.push(Component::Index(*index));
            *index += 1;
        }
        None => {}
    }
}

fn mapping_key<'a>(
    text: &str,
    parser: &mut Parser<'a, StrInput<'a>>,
    event: &Event<'a>,
    span: Span,
) -> Result<String, Box<yamlpatch::Error>> {
    if let Event::Scalar(value, ..) = event {
        return Ok(value.to_string());
    }
    let mut range = byte_range(span);
    let mut depth =
        usize::from(matches!(event, Event::SequenceStart(..) | Event::MappingStart(..)));
    while depth > 0 {
        let (event, span) = parser
            .next_event()
            .ok_or_else(|| invalid("Unclosed YAML mapping key".to_string()))?
            .map_err(|error| invalid(error.to_string()))?;
        match event {
            Event::SequenceStart(..) | Event::MappingStart(..) => depth += 1,
            Event::SequenceEnd | Event::MappingEnd => depth -= 1,
            _ => {}
        }
        range.end = byte_range(span).end;
    }
    Ok(text[range].to_string())
}

pub(super) fn changed_scalar_paths(
    text: &str,
    original: &serde_json::Value,
    target: &serde_json::Value,
) -> Result<HashSet<usize>, Box<yamlpatch::Error>> {
    let keys = crate::source_keys::SourceKeys::new(text, original)?;
    let mut changed = HashSet::new();
    for (id, paths) in scalar_paths(text)? {
        if paths
            .iter()
            .any(|path| keys.value_at(original, &path.route) != keys.value_at(target, &path.route))
        {
            changed.insert(id);
        }
    }
    Ok(changed)
}
