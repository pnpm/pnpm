//! The `npm fund` tree: packages grouped under the funding URL they share.

use super::{
    funding::funding_sources,
    report::{FundedDependencies, FundingReport},
};
use crate::cli_args::sanitize::sanitize_inline;
use owo_colors::{OwoColorize, Stream};
use serde_json::Value;
use std::collections::HashMap;

pub fn render_human(report: &FundingReport) -> String {
    let mut tree = FundingTree::default();
    let root = tree.push_package(&report.name, report.version.as_deref(), report.funding.as_ref());
    let root = root.unwrap_or_else(|| tree.push_url_less(&report.name, report.version.as_deref()));
    let children = tree.push_dependencies(&report.dependencies);
    tree.items[root].children = children;
    let mut output = String::new();
    tree.render(root, "", &mut output);
    output
}

struct Item {
    label: String,
    children: Vec<usize>,
}

#[derive(Default)]
struct FundingTree {
    items: Vec<Item>,
    item_by_url: HashMap<String, usize>,
}

impl FundingTree {
    /// The items listed under a parent. A package whose funding URL was
    /// already listed joins that URL's item, and its own funded
    /// dependencies move up to the parent.
    fn push_dependencies(&mut self, dependencies: &FundedDependencies) -> Vec<usize> {
        let mut children = Vec::new();
        for (name, package) in dependencies {
            let item = self.push_package(name, package.version.as_deref(), Some(&package.funding));
            let grandchildren = self.push_dependencies(&package.dependencies);
            match item {
                Some(item) => {
                    self.items[item].children = grandchildren;
                    children.push(item);
                }
                None => children.extend(grandchildren),
            }
        }
        children
    }

    /// A new item for the package's first funding URL, or `None` when the
    /// package joined the item of a URL listed before it or has no URL.
    fn push_package(
        &mut self,
        name: &str,
        version: Option<&str>,
        funding: Option<&Value>,
    ) -> Option<usize> {
        let source = funding_sources(funding?).into_iter().next()?;
        let url = source.url;
        let package = printable_name(name, version);
        if let Some(&item) = self.item_by_url.get(url) {
            let comma = ",".if_supports_color(Stream::Stdout, |text| text.dimmed()).to_string();
            let label = &mut self.items[item].label;
            label.push_str(&comma);
            label.push(' ');
            label.push_str(&package);
            return None;
        }
        let colored_url = sanitize_inline(&source.public_url())
            .if_supports_color(Stream::Stdout, |text| text.blue())
            .to_string();
        let item = self.push(format!("{colored_url}\n└── {package}"));
        self.item_by_url.insert(url.to_string(), item);
        Some(item)
    }

    fn push_url_less(&mut self, name: &str, version: Option<&str>) -> usize {
        self.push(printable_name(name, version))
    }

    fn push(&mut self, label: String) -> usize {
        self.items.push(Item { label, children: Vec::new() });
        self.items.len() - 1
    }

    /// Port of the `archy` renderer `npm fund` prints with.
    fn render(&self, index: usize, prefix: &str, output: &mut String) {
        let item = &self.items[index];
        let continuation = if item.children.is_empty() { ' ' } else { '│' };
        output.push_str(prefix);
        output.push_str(&item.label.replace('\n', &format!("\n{prefix}{continuation} ")));
        output.push('\n');
        for (position, &child) in item.children.iter().enumerate() {
            self.render_child(child, prefix, position + 1 == item.children.len(), output);
        }
    }

    /// A child's first line hangs off its branch; the lines below it take
    /// the prefix that continues the parent's column.
    fn render_child(&self, child: usize, prefix: &str, last: bool, output: &mut String) {
        let branch = if last { '└' } else { '├' };
        let fork = if self.items[child].children.is_empty() { '─' } else { '┬' };
        let child_prefix = format!("{prefix}{} ", if last { ' ' } else { '│' });
        output.push_str(prefix);
        output.extend([branch, '─', fork, ' ']);
        let mut rendered = String::new();
        self.render(child, &child_prefix, &mut rendered);
        output.push_str(rendered.strip_prefix(child_prefix.as_str()).unwrap_or(&rendered));
    }
}

fn printable_name(name: &str, version: Option<&str>) -> String {
    let name = sanitize_inline(name);
    match version {
        Some(version) => format!("{name}@{}", sanitize_inline(version)),
        None => name.into_owned(),
    }
}
