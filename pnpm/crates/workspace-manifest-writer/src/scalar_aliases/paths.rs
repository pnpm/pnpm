use super::{byte_range, invalid};
use serde_saphyr::granit_parser::{Event, Parser, Span, StrInput};
use yamlpath::Component;

enum Container {
    Mapping { key: Option<String> },
    Sequence { index: usize },
}

pub(super) fn scalar_paths(
    text: &str,
) -> Result<Vec<Vec<Component<'static>>>, Box<yamlpatch::Error>> {
    let mut parser = Parser::new_from_str(text);
    let mut containers = Vec::new();
    let mut path = Vec::new();
    let mut paths = Vec::new();
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| invalid(error.to_string()))?;
        if matches!(containers.last(), Some(Container::Mapping { key: None })) && event.is_node() {
            let key_text = mapping_key(text, &mut parser, event, span)?;
            if let Some(Container::Mapping { key }) = containers.last_mut() {
                *key = Some(key_text);
            }
            continue;
        }
        match event {
            event if event.is_node() => {
                push_component(&mut containers, &mut path);
                match event {
                    Event::MappingStart(..) => containers.push(Container::Mapping { key: None }),
                    Event::SequenceStart(..) => containers.push(Container::Sequence { index: 0 }),
                    _ => {
                        paths.push(path.clone());
                        path.pop();
                    }
                }
            }
            Event::MappingEnd | Event::SequenceEnd => {
                containers.pop();
                path.pop();
            }
            _ => {}
        }
    }
    Ok(paths)
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
