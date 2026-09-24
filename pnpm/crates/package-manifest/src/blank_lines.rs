use std::{collections::HashMap, iter};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Segment {
    Key(String),
    Index(usize),
}

/// The blank lines a JSON document places before its object members, keyed
/// by each member's path, so a save can put them back in front of the same
/// members.
#[derive(Debug, Clone, Default)]
pub(crate) struct BlankLines(HashMap<Vec<Segment>, usize>);

impl BlankLines {
    /// Record the blank lines of a valid JSON document.
    pub(crate) fn detect(text: &str) -> Self {
        let blank_lines = members(text)
            .into_iter()
            .filter(|member| member.line_breaks > 1)
            .map(|member| (member.path, member.line_breaks - 1))
            .collect();
        BlankLines(blank_lines)
    }

    /// Insert the recorded blank lines into `serialized`, a JSON document
    /// that puts each object member on its own line. Members that no longer
    /// exist lose their blank lines, and a single-line document stays as is.
    pub(crate) fn restore(&self, serialized: &str) -> String {
        if self.0.is_empty() {
            return serialized.to_owned();
        }
        let mut output = String::with_capacity(serialized.len() + self.0.len());
        let mut copied = 0;
        for member in members(serialized) {
            let Some(&count) = self.0.get(&member.path) else { continue };
            let Some(line_start) = serialized[..member.offset]
                .rfind('\n')
                .map(|index| index + 1)
            else {
                continue;
            };
            if !serialized[line_start..member.offset].trim().is_empty() {
                continue;
            }
            output.push_str(&serialized[copied..line_start]);
            output.extend(iter::repeat_n('\n', count));
            copied = line_start;
        }
        output.push_str(&serialized[copied..]);
        output
    }
}

struct Member {
    path: Vec<Segment>,
    /// Line breaks between the member's key and the token before it.
    line_breaks: usize,
    /// Byte offset of the key's opening quote.
    offset: usize,
}

enum Frame {
    Object { key: Option<String>, expects_key: bool },
    Array { index: usize },
}

/// Every object member of a valid JSON document, in document order.
fn members(text: &str) -> Vec<Member> {
    let bytes = text.as_bytes();
    let mut members = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut line_breaks = 0;
    let mut position = 0;
    while position < bytes.len() {
        let byte = bytes[position];
        match byte {
            b'\n' => {
                line_breaks += 1;
                position += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                position += 1;
                continue;
            }
            b'{' => frames.push(Frame::Object { key: None, expects_key: true }),
            b'[' => frames.push(Frame::Array { index: 0 }),
            b'}' | b']' => {
                frames.pop();
            }
            b',' => match frames.last_mut() {
                Some(Frame::Object { expects_key, .. }) => *expects_key = true,
                Some(Frame::Array { index }) => *index += 1,
                None => {}
            },
            b'"' => {
                let end = string_end(bytes, position);
                if let Some(Frame::Object { expects_key: true, .. }) = frames.last() {
                    let raw = &text[position..end];
                    let key =
                        serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned());
                    let mut path = path_of(&frames[..frames.len() - 1]);
                    path.push(Segment::Key(key.clone()));
                    members.push(Member { path, line_breaks, offset: position });
                    if let Some(Frame::Object { key: current, expects_key }) = frames.last_mut() {
                        *current = Some(key);
                        *expects_key = false;
                    }
                }
                line_breaks = 0;
                position = end;
                continue;
            }
            _ => {}
        }
        line_breaks = 0;
        position += 1;
    }
    members
}

/// The byte offset just past the string literal whose opening quote is at
/// `start`.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut position = start + 1;
    while position < bytes.len() {
        match bytes[position] {
            b'\\' => position += 2,
            b'"' => return position + 1,
            _ => position += 1,
        }
    }
    bytes.len()
}

fn path_of(frames: &[Frame]) -> Vec<Segment> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            Frame::Object { key, .. } => key.clone().map(Segment::Key),
            Frame::Array { index } => Some(Segment::Index(*index)),
        })
        .collect()
}
