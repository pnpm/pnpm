use std::collections::HashMap;

/// `note` as plain text for the terminal: control and formatting characters
/// a registry let through are removed, and each inline Markdown link or
/// image shows its target next to its text, so no target is hidden from the
/// reader. Nothing is fetched and no HTML is rendered.
#[must_use]
pub fn render_note(note: &str) -> String {
    let sanitized = pnpm_text_sanitize::sanitize(note);
    render_links(&sanitized, MAX_LINK_DEPTH).trim_end().to_string()
}

/// How deeply link labels nested in link labels are rendered. A label
/// deeper than this is shown as written.
const MAX_LINK_DEPTH: usize = 8;

/// `text` with each inline link rendered, its labels down to `depth` levels.
/// Brackets are matched once up front, so a note with many unmatched
/// brackets takes linear time.
fn render_links(text: &str, depth: usize) -> String {
    let brackets = Brackets::new(text);
    let mut output = String::with_capacity(text.len());
    let mut position = 0;
    while let Some(offset) = text[position..].find('[') {
        let open = position + offset;
        let before = &text[position..open];
        let image_text = before.strip_suffix('!');
        output.push_str(image_text.unwrap_or(before));
        let Some(link) = InlineLink::parse(text, open, &brackets) else {
            if image_text.is_some() {
                output.push('!');
            }
            output.push('[');
            position = open + 1;
            continue;
        };
        link.render_into(&mut output, image_text.is_some(), depth);
        position = link.end;
    }
    output.push_str(&text[position..]);
    output
}

/// The closing `]` and `)` of every `[` and `(` that has one, by byte index,
/// nested pairs counted.
struct Brackets {
    square: HashMap<usize, usize>,
    round: HashMap<usize, usize>,
}

impl Brackets {
    fn new(text: &str) -> Self {
        Self { square: matching_pairs(text, '[', ']'), round: matching_pairs(text, '(', ')') }
    }
}

fn matching_pairs(text: &str, open: char, close: char) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    let mut opened = Vec::new();
    for (index, character) in text.char_indices() {
        if character == open {
            opened.push(index);
        } else if character == close
            && let Some(start) = opened.pop()
        {
            pairs.insert(start, index);
        }
    }
    pairs
}

/// A `[label](destination "title")` link.
struct InlineLink<'a> {
    label: &'a str,
    target: &'a str,
    /// The byte index just past the link.
    end: usize,
}

impl<'a> InlineLink<'a> {
    /// The link whose `[` is at `open` in `text`.
    fn parse(text: &'a str, open: usize, brackets: &Brackets) -> Option<Self> {
        let label_end = *brackets.square.get(&open)?;
        let destination_start = label_end + 1;
        if !text[destination_start..].starts_with('(') {
            return None;
        }
        let destination_end = *brackets.round.get(&destination_start)?;
        let destination = text[destination_start + 1..destination_end].trim();
        let target = destination
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let target = target
            .strip_prefix('<')
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(target);
        Some(Self { label: &text[open + 1..label_end], target, end: destination_end + 1 })
    }

    fn render_into(&self, output: &mut String, is_image: bool, depth: usize) {
        if is_image {
            output.push_str("image: ");
        }
        let label = match depth.checked_sub(1) {
            Some(depth) => render_links(self.label, depth),
            None => self.label.to_owned(),
        };
        let label = label.trim_end();
        if label.is_empty() || label == self.target {
            output.push_str(self.target);
        } else {
            output.push_str(label);
            output.push_str(" (");
            output.push_str(self.target);
            output.push(')');
        }
    }
}

#[cfg(test)]
mod tests;
