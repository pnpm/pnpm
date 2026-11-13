//! A [`Matrix`] as a self-contained HTML page.

use crate::dsm::{Dependency, Group, Mark, Matrix};
use std::{collections::BTreeSet, fmt::Write as _};

/// The number of shades a co-change count maps to.
const SHADES: usize = 5;

/// How many pairs that change together without a dependency are listed.
const LISTED_PAIRS: usize = 40;

const STYLE: &str = "\
:root{color-scheme:light dark;--fg:#1f2328;--bg:#fff;--line:#d8dee4;--rule:#6e7781;\
--heat:9,105,218;--up:#cf222e;--ex:#bc4c00;--diag:#d0d7de}\
@media (prefers-color-scheme:dark){:root{--fg:#e6edf3;--bg:#0d1117;--line:#262c36;\
--rule:#8b949e;--heat:56,139,253;--up:#ff7b72;--ex:#f0883e;--diag:#3d444d}}\
body{font:13px/1.4 system-ui,sans-serif;color:var(--fg);background:var(--bg);margin:16px}\
table{border-collapse:collapse}\
.dsm td,.dsm th{border:1px solid var(--line);min-width:15px;height:15px;padding:0;\
text-align:center;font-size:10px}\
.dsm thead th{position:sticky;top:0;background:var(--bg)}\
.dsm th.n{text-align:left;padding:0 6px;white-space:nowrap;font-weight:normal}\
.dsm th.g{writing-mode:vertical-rl;transform:rotate(180deg);padding:4px 2px}\
.dsm tbody,.dsm colgroup{border:2px solid var(--rule)}\
.d{background:var(--diag)}.u{color:var(--up);font-weight:bold}.e{color:var(--ex)}.t{opacity:.6}\
.h1{background:rgba(var(--heat),.12)}.h2{background:rgba(var(--heat),.22)}\
.h3{background:rgba(var(--heat),.32)}.h4{background:rgba(var(--heat),.44)}\
.h5{background:rgba(var(--heat),.56)}\
.list td,.list th{padding:2px 8px;text-align:left;border-bottom:1px solid var(--line)}";

pub fn render(matrix: &Matrix) -> String {
    let title = escape(&matrix.title);
    let mut page = format!(
        "<!doctype html>\n<html lang=en>\n<meta charset=utf-8>\n<title>{title}</title>\n\
         <style>{STYLE}</style>\n<h1>{title}</h1>\n",
    );
    push_summary(&mut page, matrix);
    push_matrix(&mut page, matrix);
    push_independent_pairs(&mut page, matrix);
    page
}

fn push_summary(page: &mut String, matrix: &Matrix) {
    let co_change = &matrix.co_change;
    let span = co_change.span
        .as_ref()
        .map(|(oldest, newest)| format!(", from {oldest} to {newest},"))
        .unwrap_or_default();
    let marks = matrix.dependencies
        .values()
        .map(|dependency| dependency.mark)
        .collect::<BTreeSet<_>>();
    let legend = marks
        .into_iter()
        .map(legend_entry)
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(
        page,
        "<p>A row depends on a column where the cell holds a mark: {legend}. The shade of a \
         cell grows with how many of the latest {} commits{span} changed both. The {} \
         that changed more than {} of the rows are left out of the pair counts. Hover a cell \
         for the details.</p>",
        co_change.commits,
        commits(co_change.wide_commits),
        co_change.max_units,
    );
}

fn legend_entry(mark: Mark) -> String {
    let (symbol, class) = mark_symbol(mark);
    let meaning = match mark {
        Mark::TestsOnly => "a dev-dependency only",
        Mark::Linked => "a dependency",
        Mark::Excepted => "an upward dependency accepted by an exception",
        Mark::Upward => "an upward dependency",
    };
    match class {
        Some(class) => format!("<span class={class}>{symbol}</span> {meaning}"),
        None => format!("{symbol} {meaning}"),
    }
}

fn push_matrix(page: &mut String, matrix: &Matrix) {
    let members = matrix.members().collect::<Vec<_>>();
    let max = max_pair_count(matrix, &members);
    let grid = Grid { matrix, members, max };
    page.push_str("<table class=dsm>\n");
    grid.push_header(page);
    let mut first_index = 1;
    for group in &matrix.groups {
        grid.push_group(page, group, first_index);
        first_index += group.members.len();
    }
    page.push_str("</table>\n");
}

fn max_pair_count(matrix: &Matrix, members: &[&String]) -> usize {
    let together = members
        .iter()
        .flat_map(|row| {
            members
                .iter()
                .map(|column| matrix.co_change.together(row, column))
        });
    together.max().unwrap_or_default()
}

/// The rows and columns of a [`Matrix`] and the highest co-change count
/// among them, which sets the darkest shade.
struct Grid<'matrix> {
    matrix: &'matrix Matrix,
    members: Vec<&'matrix String>,
    max: usize,
}

impl Grid<'_> {
    /// One `colgroup` per group, so the layer boundaries are drawn between
    /// columns, and the column numbers.
    fn push_header(&self, page: &mut String) {
        page.push_str("<colgroup span=2></colgroup>");
        for group in &self.matrix.groups {
            let _ = write!(page, "<colgroup span={}></colgroup>", group.members.len());
        }
        page.push_str("\n<thead><tr><th><th>");
        for (index, name) in self.members.iter().enumerate() {
            let _ = write!(page, r#"<th title="{}">{}"#, escape(name), index + 1);
        }
        page.push_str("</thead>\n");
    }

    fn push_group(&self, page: &mut String, group: &Group, first_index: usize) {
        page.push_str("<tbody>\n<tr>");
        let _ = write!(page, "<th class=g rowspan={}>{}", group.members.len(), escape(&group.name));
        for (offset, row) in group.members.iter().enumerate() {
            if offset > 0 {
                page.push_str("<tr>");
            }
            let _ = write!(page, "<th class=n>{} {}", first_index + offset, escape(row));
            for column in &self.members {
                self.push_cell(page, row, column);
            }
            page.push('\n');
        }
        page.push_str("</tbody>\n");
    }

    fn push_cell(&self, page: &mut String, row: &str, column: &str) {
        let co_change = &self.matrix.co_change;
        if row == column {
            let changes = commits(co_change.changes_of(row));
            let _ = write!(page, r#"<td class=d title="{}: changed in {changes}">"#, escape(row));
            return;
        }
        let dependency = self.matrix.dependencies.get(&(row.to_owned(), column.to_owned()));
        let together = co_change.together(row, column);
        if dependency.is_none() && together == 0 {
            page.push_str("<td>");
            return;
        }
        let mut classes = Vec::new();
        let mut title = format!("{row} → {column}");
        if together > 0 {
            classes.push(format!("h{}", shade(together, self.max)));
            let (row_changes, column_changes) =
                (co_change.changes_of(row), co_change.changes_of(column));
            let _ = write!(
                title,
                "\nchanged together in {} ({row}: {row_changes}, {column}: {column_changes})",
                commits(together),
            );
        }
        let symbol = dependency.map_or("", |dependency| {
            let (symbol, class) = mark_symbol(dependency.mark);
            classes.extend(class.map(str::to_owned));
            title.push_str(&describe(dependency));
            symbol
        });
        push_cell_tag(page, &classes, &title, symbol);
    }
}

fn push_cell_tag(page: &mut String, classes: &[String], title: &str, symbol: &str) {
    page.push_str("<td");
    if !classes.is_empty() {
        let _ = write!(page, r#" class="{}""#, classes.join(" "));
    }
    let _ = write!(page, r#" title="{}">{symbol}"#, escape(title));
}

/// The shade, from 1 to [`SHADES`], of a pair that changed together in
/// `count` of the commits, where `max` is the highest count of any pair.
/// The shade grows with the square root of the count, so pairs well below
/// the highest count still show.
pub fn shade(count: usize, max: usize) -> usize {
    let is_within = |shade: usize| count * SHADES * SHADES <= max * shade * shade;
    (1..=SHADES).find(|&shade| is_within(shade)).unwrap_or(SHADES)
}

const fn mark_symbol(mark: Mark) -> (&'static str, Option<&'static str>) {
    match mark {
        Mark::TestsOnly => ("○", Some("t")),
        Mark::Linked => ("●", None),
        Mark::Excepted => ("▲", Some("e")),
        Mark::Upward => ("▲", Some("u")),
    }
}

fn describe(dependency: &Dependency) -> String {
    let direction = match dependency.mark {
        Mark::TestsOnly | Mark::Linked => "",
        Mark::Excepted => ", upward, accepted by an exception",
        Mark::Upward => ", upward",
    };
    format!("\ndepends: {}{direction}", dependency.detail)
}

fn push_independent_pairs(page: &mut String, matrix: &Matrix) {
    let pairs = matrix.independent_pairs();
    page.push_str("<h2>Changed together without a dependency</h2>\n");
    if pairs.is_empty() {
        page.push_str("<p>None.</p>\n");
        return;
    }
    let _ = writeln!(
        page,
        "<p>Up to {LISTED_PAIRS} pairs that depend on each other in neither direction, the \
         ones that changed together most often first.</p>\n<table class=list>\n\
         <tr><th>Pair<th>Commits together<th>Commits that changed each",
    );
    let co_change = &matrix.co_change;
    for (left, right, count) in pairs.into_iter().take(LISTED_PAIRS) {
        let _ = writeln!(
            page,
            "<tr><td>{} and {}<td>{count}<td>{}, {}",
            escape(left),
            escape(right),
            co_change.changes_of(left),
            co_change.changes_of(right),
        );
    }
    page.push_str("</table>\n");
}

fn commits(count: usize) -> String {
    let noun = if count == 1 { "commit" } else { "commits" };
    format!("{count} {noun}")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
