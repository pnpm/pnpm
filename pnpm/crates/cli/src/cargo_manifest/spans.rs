use std::ops::Range;
use toml::{
    Spanned,
    de::{DeString, DeTable, DeValue},
};

pub(super) fn collect_header_spans(
    table: &DeTable<'_>,
    contents: &str,
    out: &mut Vec<Range<usize>>,
) {
    for value in table.values() {
        collect_value_header_spans(value, contents, out);
    }
}

fn collect_value_header_spans(
    value: &Spanned<DeValue<'_>>,
    contents: &str,
    out: &mut Vec<Range<usize>>,
) {
    match value.get_ref() {
        DeValue::Table(table) => {
            let span = value.span();
            if is_explicit_header(contents, &span) {
                out.push(span);
            }
            collect_header_spans(table, contents, out);
        }
        DeValue::Array(items) => {
            for item in items {
                collect_value_header_spans(item, contents, out);
            }
        }
        _ => {}
    }
}

pub(super) fn lookup_table_path<'a, 'i>(
    doc: &'a Spanned<DeTable<'i>>,
    table_path: &str,
) -> Option<&'a Spanned<DeValue<'i>>> {
    let mut segments = table_path.split('.');
    let first = segments.next()?;
    let (_, mut current) = find_entry(doc.get_ref(), first)?;
    for segment in segments {
        let table = current.get_ref().as_table()?;
        (_, current) = find_entry(table, segment)?;
    }
    Some(current)
}

pub(super) fn find_entry<'a, 'i>(
    table: &'a DeTable<'i>,
    name: &str,
) -> Option<(&'a Spanned<DeString<'i>>, &'a Spanned<DeValue<'i>>)> {
    table
        .iter()
        .find(|(key, _)| key.get_ref().as_ref() == name)
}

pub(super) fn is_explicit_header(contents: &str, span: &Range<usize>) -> bool {
    contents
        .get(span.clone())
        .is_some_and(|slice| slice.starts_with('['))
}

pub(super) fn is_inline_table(contents: &str, span: &Range<usize>) -> bool {
    contents
        .get(span.clone())
        .is_some_and(|slice| slice.starts_with('{'))
}

pub(super) fn section_end(
    contents: &str,
    header_spans: &[Range<usize>],
    header_span: &Range<usize>,
) -> usize {
    header_spans
        .iter()
        .find(|span| span.start > header_span.start)
        .map_or(contents.len(), |next| line_start(contents, next.start))
}

pub(super) fn dotted_prefix<'a>(contents: &'a str, span: &Range<usize>) -> &'a str {
    let line_begin = line_start(contents, span.start);
    contents[line_begin..span.end].trim_start_matches([' ', '\t'])
}

pub(super) fn line_start(contents: &str, pos: usize) -> usize {
    contents[..pos]
        .rfind('\n')
        .map_or(0, |nl| nl + 1)
}

pub(super) fn end_of_line(contents: &str, pos: usize) -> usize {
    contents[pos..]
        .find('\n')
        .map_or(contents.len(), |nl| pos + nl + 1)
}

pub(super) fn full_line_range(contents: &str, span: Range<usize>) -> Range<usize> {
    line_start(contents, span.start)..end_of_line(contents, span.end)
}

pub(super) fn max_value_end(table: &DeTable<'_>) -> Option<usize> {
    table
        .values()
        .map(|val| val.span().end)
        .max()
}

pub(super) fn insert_line_at(contents: &str, offset: usize, line: &str) -> String {
    let prefix = &contents[..offset];
    let newline = if !prefix.is_empty() && !prefix.ends_with('\n') { "\n" } else { "" };
    format!("{prefix}{newline}{line}\n{}", &contents[offset..])
}

pub(super) fn insert_into_inline_table(
    contents: &str,
    table_span: &Range<usize>,
    subtable: &DeTable<'_>,
    entry_text: &str,
) -> String {
    let Some(first_start) = subtable
        .keys()
        .map(|key| key.span().start)
        .min()
    else {
        return replace_range(contents, table_span.clone(), &format!("{{ {entry_text} }}"));
    };
    replace_range(contents, first_start..first_start, &format!("{entry_text}, "))
}

pub(super) fn inline_entry_removal_range(
    contents: &str,
    table_span: &Range<usize>,
    subtable: &DeTable<'_>,
    entry_span: Range<usize>,
) -> Range<usize> {
    let after = skip_ws_and_comments(contents, entry_span.end, table_span.end);
    if contents.as_bytes().get(after) == Some(&b',') {
        let mut end = after + 1;
        while matches!(contents.as_bytes().get(end), Some(b' ' | b'\t')) {
            end += 1;
        }
        return entry_span.start..end;
    }
    let prev_end = subtable
        .values()
        .map(|val| val.span().end)
        .filter(|&end| end <= entry_span.start)
        .max()
        .unwrap_or(table_span.start + 1);
    let comma = skip_ws_and_comments(contents, prev_end, entry_span.start);
    comma..entry_span.end
}

fn skip_ws_and_comments(contents: &str, mut pos: usize, limit: usize) -> usize {
    let bytes = contents.as_bytes();
    while pos < limit {
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
        } else if bytes[pos] == b'#' {
            while pos < limit && bytes[pos] != b'\n' {
                pos += 1;
            }
        } else {
            break;
        }
    }
    pos
}

pub(super) fn replace_range(contents: &str, range: Range<usize>, replacement: &str) -> String {
    format!("{}{replacement}{}", &contents[..range.start], &contents[range.end..])
}

pub(super) fn apply_two_edits(
    contents: &str,
    first: (Range<usize>, &str),
    second: (Range<usize>, &str),
) -> String {
    let ((earlier, earlier_text), (later, later_text)) =
        if first.0.start <= second.0.start { (first, second) } else { (second, first) };
    let updated = replace_range(contents, later, later_text);
    replace_range(&updated, earlier, earlier_text)
}
