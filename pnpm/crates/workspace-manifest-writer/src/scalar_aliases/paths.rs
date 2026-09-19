use super::{byte_range, invalid};
use serde_saphyr::granit_parser::{Event, Parser, Span, StrInput};
use yamlpath::Component;

enum Container {
    Mapping { key: Option<String> },
    Sequence { index: usize },
}

#[derive(Default)]
struct PathCollector {
    containers: Vec<Container>,
    path: Vec<Component<'static>>,
    paths: Vec<Vec<Component<'static>>>,
}

pub(super) fn scalar_paths(
    text: &str,
) -> Result<Vec<Vec<Component<'static>>>, Box<yamlpatch::Error>> {
    let mut parser = Parser::new_from_str(text);
    let mut collector = PathCollector::default();
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| invalid(error.to_string()))?;
        if event.is_node()
            && let Some(Container::Mapping { key: key @ None }) = collector
                .containers
                .last_mut()
        {
            *key = Some(mapping_key(text, &mut parser, event, span)?);
            continue;
        }
        collector.visit(event);
    }
    Ok(collector.paths)
}

impl PathCollector {
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
                        self.paths.push(self.path.clone());
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
    event: Event<'a>,
    span: Span,
) -> Result<String, Box<yamlpatch::Error>> {
    if let Event::Scalar(value, ..) = event {
        return Ok(value.into_owned());
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
