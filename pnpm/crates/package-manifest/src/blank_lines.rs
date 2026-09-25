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
#[derive(Debug, Default, Clone)]
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
    let mut scanner = Scanner { text, frames: Vec::new(), members: Vec::new(), line_breaks: 0 };
    let mut position = 0;
    while position < text.len() {
        position = scanner.step(position);
    }
    scanner.members
}

struct Scanner<'a> {
    text: &'a str,
    frames: Vec<Frame>,
    members: Vec<Member>,
    /// Line breaks since the last token.
    line_breaks: usize,
}

impl Scanner<'_> {
    /// Consume the byte at `position` and return where scanning resumes.
    fn step(&mut self, position: usize) -> usize {
        match self.text.as_bytes()[position] {
            b'\n' => {
                self.line_breaks += 1;
                return position + 1;
            }
            b' ' | b'\t' | b'\r' => return position + 1,
            b'"' => {
                let end = string_end(self.text.as_bytes(), position);
                self.string(position, end);
                self.line_breaks = 0;
                return end;
            }
            b'{' => self.frames.push(Frame::Object { key: None, expects_key: true }),
            b'[' => self.frames.push(Frame::Array { index: 0 }),
            b'}' | b']' => {
                self.frames.pop();
            }
            b',' => self.next_entry(),
            _ => {}
        }
        self.line_breaks = 0;
        position + 1
    }

    fn next_entry(&mut self) {
        match self.frames.last_mut() {
            Some(Frame::Object { expects_key, .. }) => *expects_key = true,
            Some(Frame::Array { index }) => *index += 1,
            None => {}
        }
    }

    /// Record the string literal at `start..end` as a member if it is a key.
    fn string(&mut self, start: usize, end: usize) {
        let Some((
            Frame::Object {
                key,
                expects_key: expects_key @ true,
            },
            parents,
        )) = self.frames.split_last_mut()
        else {
            return;
        };
        let raw = &self.text[start..end];
        let name = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned());
        let mut path = path_of(parents);
        path.push(Segment::Key(name.clone()));
        self.members.push(Member { path, line_breaks: self.line_breaks, offset: start });
        *key = Some(name);
        *expects_key = false;
    }
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
