//! A block of a text file that pnpm owns: the lines between a `# <name>`
//! line and a `# <name> end` line. Lines outside the block belong to the
//! user and are never touched.

use std::ops::Range;

/// One `# <name>` ... `# <name> end` block of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// From the start of the opening marker to the end of the closing
    /// marker, excluding its line break.
    pub range: Range<usize>,
    /// The lines between the markers, without the final line break.
    pub body: String,
}

/// Every `# <name>` ... `# <name> end` block of `content`, in order. A
/// block is closed by the first end marker after its last opening marker,
/// so an opening marker without an end marker forms no block.
#[must_use]
pub fn find_sections(content: &str, name: &str) -> Vec<Section> {
    let start_marker = format!("# {name}");
    let end_marker = format!("# {name} end");
    let mut sections = Vec::new();
    let mut last_start = None;
    let mut offset = 0;

    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let trimmed = line.trim_end_matches(['\r', '\n', ' ', '\t']);

        if trimmed == start_marker {
            last_start = Some((line_start, offset));
        } else if trimmed == end_marker
            && let Some((start, inner)) = last_start.take()
        {
            sections.push(complete_section(content, start, inner, line_start, line));
        }
    }

    sections
}

/// `body` between the markers of the block `name`.
#[must_use]
pub fn wrap_section(name: &str, body: &str) -> String {
    format!("# {name}\n{body}\n# {name} end")
}

fn complete_section(
    content: &str,
    start_offset: usize,
    inner_start: usize,
    line_start: usize,
    line: &str,
) -> Section {
    let inner_len = content[inner_start..line_start]
        .trim_end_matches(['\r', '\n'])
        .len();
    let body = content[inner_start..inner_start + inner_len].to_string();
    let marker_len = line
        .trim_end_matches(['\r', '\n'])
        .len();
    Section { range: start_offset..line_start + marker_len, body }
}

#[cfg(test)]
mod tests;
