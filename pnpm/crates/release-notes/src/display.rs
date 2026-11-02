/// `note` as plain text for the terminal: control and formatting characters
/// a registry let through are removed, and each inline Markdown link or
/// image shows its target next to its text, so no target is hidden from the
/// reader. Nothing is fetched and no HTML is rendered.
#[must_use]
pub fn render_note(note: &str) -> String {
    let sanitized = pnpm_text_sanitize::sanitize(note);
    let mut output = String::with_capacity(sanitized.len());
    let mut rest: &str = &sanitized;
    while let Some(open) = rest.find('[') {
        let (before, from_bracket) = rest.split_at(open);
        let image_text = before.strip_suffix('!');
        output.push_str(image_text.unwrap_or(before));
        let Some(link) = InlineLink::parse(from_bracket) else {
            if image_text.is_some() {
                output.push('!');
            }
            output.push('[');
            rest = &from_bracket[1..];
            continue;
        };
        link.render_into(&mut output, image_text.is_some());
        rest = &from_bracket[link.consumed..];
    }
    output.push_str(rest);
    output.trim_end().to_string()
}

/// A `[label](destination "title")` link at the start of a string.
struct InlineLink<'a> {
    label: &'a str,
    target: &'a str,
    consumed: usize,
}

impl<'a> InlineLink<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        let label_end = matching_close(text, '[', ']')?;
        let after_label = &text[label_end + 1..];
        if !after_label.starts_with('(') {
            return None;
        }
        let destination_end = matching_close(after_label, '(', ')')?;
        let destination = after_label[1..destination_end].trim();
        let target = destination
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let target = target
            .strip_prefix('<')
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(target);
        Some(Self {
            label: &text[1..label_end],
            target,
            consumed: label_end + 1 + destination_end + 1,
        })
    }

    fn render_into(&self, output: &mut String, is_image: bool) {
        if is_image {
            output.push_str("image: ");
        }
        let label = render_note(self.label);
        if label.is_empty() || label == self.target {
            output.push_str(self.target);
        } else {
            output.push_str(&label);
            output.push_str(" (");
            output.push_str(self.target);
            output.push(')');
        }
    }
}

/// The byte index of the bracket that closes the one `text` starts with,
/// counting nested pairs.
fn matching_close(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        if character == open {
            depth += 1;
        } else if character == close {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
