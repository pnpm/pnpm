use super::{
    DependencyGroup, IntoDiagnostic, OutdatedInWorkspace, OutdatedPackage, SortBy, Stream, Version,
    Write, sanitize_inline,
};
use owo_colors::OwoColorize;

pub(super) fn write_output(output: &str) -> miette::Result<()> {
    let mut stdout = std::io::stdout();
    writeln!(stdout, "{output}").into_diagnostic()?;
    stdout.flush().into_diagnostic()
}

/// The kind of semver bump from `current` to `target`. Drives the default
/// sort order and the colorized highlight in the `Latest` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    None,
    Fix,
    Feature,
    Breaking,
    Unknown,
}

pub(super) fn classify(current: &Version, target: &Version) -> Change {
    if current == target {
        Change::None
    } else if target.major != current.major {
        Change::Breaking
    } else if target.minor != current.minor {
        Change::Feature
    } else if target.patch != current.patch {
        Change::Fix
    } else {
        Change::Unknown
    }
}

/// Ascending sort priority for the default (non-`--sort-by name`) order:
/// no-change, fix, feature, breaking, then unknown last.
fn change_priority(change: Change) -> u8 {
    match change {
        Change::None => 0,
        Change::Fix => 1,
        Change::Feature => 2,
        Change::Breaking => 3,
        Change::Unknown => 4,
    }
}

pub(super) fn sort_outdated(outdated: &mut [OutdatedPackage], sort_by: Option<SortBy>) {
    outdated.sort_by(|left, right| compare_outdated(left, right, sort_by));
}

pub(super) fn sort_workspace_outdated(outdated: &mut [OutdatedInWorkspace]) {
    outdated.sort_by(|left, right| {
        compare_outdated(&left.package, &right.package, None).then_with(|| {
            dependency_group_priority(left.package.belongs_to)
                .cmp(&dependency_group_priority(right.package.belongs_to))
        })
    });
}

fn dependency_group_priority(group: DependencyGroup) -> u8 {
    match group {
        DependencyGroup::Optional => 0,
        DependencyGroup::Prod => 1,
        DependencyGroup::Dev => 2,
        DependencyGroup::Peer => 3,
    }
}

fn compare_outdated(
    left: &OutdatedPackage,
    right: &OutdatedPackage,
    sort_by: Option<SortBy>,
) -> std::cmp::Ordering {
    if sort_by == Some(SortBy::Name) {
        return left.package_name.cmp(&right.package_name);
    }
    let by_change = change_priority(classify(&left.current, &left.target))
        .cmp(&change_priority(classify(&right.current, &right.target)));
    by_change
        .then_with(|| left.package_name.cmp(&right.package_name))
        .then_with(|| left.current.to_string().cmp(&right.current.to_string()))
}

pub(super) fn render_table(outdated: &[OutdatedPackage], long: bool) -> String {
    if outdated.is_empty() {
        return String::new();
    }
    use tabled::builder::Builder;
    use tabled::settings::Style;

    let mut header: Vec<String> =
        ["Package", "Current", "Latest"].iter().map(|h| bright_blue(h)).collect();
    if long {
        header.push(bright_blue("Details"));
    }

    let mut builder = Builder::default();
    builder.push_record(header);
    for pkg in outdated {
        let mut row = vec![render_package_name(pkg), pkg.current.to_string(), render_latest(pkg)];
        if long {
            row.push(render_details(pkg));
        }
        builder.push_record(row);
    }
    let mut table = builder.build();
    table.with(Style::modern());
    table.to_string()
}

pub(super) fn render_list(outdated: &[OutdatedPackage], long: bool) -> String {
    outdated
        .iter()
        .map(|pkg| {
            let mut info = format!(
                "{}\n{} {} {}",
                bold(&render_package_name(pkg)),
                pkg.current,
                grey("=>"),
                render_latest(pkg),
            );
            if long {
                let details = render_details(pkg);
                if !details.is_empty() {
                    info.push('\n');
                    info.push_str(&details);
                }
            }
            info
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn render_json(outdated: &[OutdatedPackage], long: bool) -> String {
    let mut map = serde_json::Map::new();
    for pkg in outdated {
        let dependency_type: &'static str =
            if pkg.github_action { "githubAction" } else { pkg.belongs_to.into() };
        let mut entry = serde_json::json!({
            "current": pkg.current.to_string(),
            "latest": pkg.target.to_string(),
            "wanted": pkg.wanted.to_string(),
            "isDeprecated": pkg.deprecated.is_some(),
            "dependencyType": dependency_type,
        });
        if long {
            entry["latestManifest"] = serde_json::json!({
                "name": pkg.package_name,
                "version": pkg.target.to_string(),
                "deprecated": pkg.deprecated,
                "homepage": pkg.homepage,
            });
        }
        map.insert(pkg.package_name.clone(), entry);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize outdated report to JSON")
}

/// A dependency shared by every project of a large workspace lists all of
/// them in one `Dependents` cell, so that column is the only one that can
/// push the table past any terminal. It wraps at this many columns instead.
pub(super) const DEPENDENTS_COLUMN_WIDTH: usize = 30;

pub(super) fn render_recursive_table(outdated: &[OutdatedInWorkspace], long: bool) -> String {
    if outdated.is_empty() {
        return String::new();
    }
    use tabled::builder::Builder;
    use tabled::settings::object::Columns;
    use tabled::settings::{Modify, Style, Width};

    const DEPENDENTS_COLUMN: usize = 3;

    let mut header: Vec<String> = ["Package", "Current", "Latest", "Dependents"]
        .iter()
        .map(|heading| bright_blue(heading))
        .collect();
    if long {
        header.push(bright_blue("Details"));
    }
    let mut builder = Builder::default();
    builder.push_record(header);
    for entry in outdated {
        let mut row = vec![
            render_package_name(&entry.package),
            entry.package.current.to_string(),
            render_latest(&entry.package),
            render_dependents(entry),
        ];
        if long {
            row.push(render_details(&entry.package));
        }
        builder.push_record(row);
    }
    let mut table = builder.build();
    table.with(Style::modern());
    table.with(
        Modify::new(Columns::one(DEPENDENTS_COLUMN))
            .with(Width::wrap(DEPENDENTS_COLUMN_WIDTH).keep_words(true)),
    );
    table.to_string()
}

pub(super) fn render_recursive_list(outdated: &[OutdatedInWorkspace], long: bool) -> String {
    outdated
        .iter()
        .map(|entry| {
            let package = &entry.package;
            let label = if entry.dependents.len() == 1 { "Dependent:" } else { "Dependents:" };
            let mut info = format!(
                "{}\n{} {} {}\n{} {}",
                bold(&render_package_name(package)),
                package.current,
                grey("=>"),
                render_latest(package),
                bold(label),
                render_dependents(entry),
            );
            if long {
                let details = render_details(package);
                if !details.is_empty() {
                    info.push('\n');
                    info.push_str(&details);
                }
            }
            info
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn render_recursive_json(outdated: &[OutdatedInWorkspace], long: bool) -> String {
    let mut map = serde_json::Map::new();
    for entry in outdated {
        let package = &entry.package;
        let dependency_type: &'static str =
            if package.github_action { "githubAction" } else { package.belongs_to.into() };
        let mut value = serde_json::json!({
            "current": package.current.to_string(),
            "latest": package.target.to_string(),
            "wanted": package.current.to_string(),
            "isDeprecated": package.deprecated.is_some(),
            "dependencyType": dependency_type,
            "dependentPackages": entry.dependents.iter().map(|dependent| serde_json::json!({
                "name": dependent.name,
                "location": dependent.location.to_string_lossy(),
            })).collect::<Vec<_>>(),
        });
        if long {
            value["latestManifest"] = serde_json::json!({
                "name": package.package_name,
                "version": package.target.to_string(),
                "deprecated": package.deprecated,
                "homepage": package.homepage,
            });
        }
        map.insert(package.package_name.clone(), value);
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .expect("serialize recursive outdated report to JSON")
}

pub(super) fn render_dependents(entry: &OutdatedInWorkspace) -> String {
    let mut names: Vec<String> = entry
        .dependents
        .iter()
        .map(|dependent| sanitize_inline(&dependent.name).into_owned())
        .collect();
    names.sort_unstable();
    names.join(", ")
}

fn render_package_name(pkg: &OutdatedPackage) -> String {
    if pkg.github_action {
        return format!("{} {}", pkg.package_name, dimmed("(github action)"));
    }
    match pkg.belongs_to {
        DependencyGroup::Dev => format!("{} {}", pkg.package_name, dimmed("(dev)")),
        DependencyGroup::Optional => format!("{} {}", pkg.package_name, dimmed("(optional)")),
        _ => pkg.package_name.clone(),
    }
}

/// The `target` version with the segment that changed highlighted, and
/// nothing else — the form the interactive update list shows, where a
/// deprecation is not part of the row.
pub(crate) fn colorize_target(pkg: &OutdatedPackage) -> String {
    let change = classify(&pkg.current, &pkg.target);
    if change == Change::None {
        return pkg.target.to_string();
    }
    colorize_version(&pkg.target, change)
}

pub(super) fn render_latest(pkg: &OutdatedPackage) -> String {
    let change = classify(&pkg.current, &pkg.target);
    if change == Change::None {
        return if pkg.deprecated.is_some() {
            red_bold("Deprecated")
        } else {
            pkg.target.to_string()
        };
    }
    let colored = colorize_version(&pkg.target, change);
    if pkg.deprecated.is_some() { format!("{colored} {}", red("(deprecated)")) } else { colored }
}

/// Highlight the version segment that changed: the whole string for a
/// breaking bump, from the minor field for a feature bump, from the patch
/// field for a fix.
fn colorize_version(version: &Version, change: Change) -> String {
    let text = version.to_string();
    let split = match change {
        Change::Breaking => 0,
        Change::Feature => text.find('.').map_or(0, |i| i + 1),
        Change::Fix => {
            text.find('.').and_then(|i| text[i + 1..].find('.').map(|j| i + 1 + j + 1)).unwrap_or(0)
        }
        // Nothing is highlighted for an `unknown` (or no) change, so the
        // version renders plain.
        Change::None | Change::Unknown => return text,
    };
    let (head, tail) = text.split_at(split);
    let painted = match change {
        Change::Breaking => red(tail),
        Change::Feature => yellow(tail),
        Change::Fix => green(tail),
        Change::None | Change::Unknown => tail.to_string(),
    };
    format!("{head}{painted}")
}

fn render_details(pkg: &OutdatedPackage) -> String {
    let mut outputs = Vec::new();
    if let Some(reason) = &pkg.deprecated
        && !reason.is_empty()
    {
        outputs.push(red(reason));
    }
    if let Some(homepage) = &pkg.homepage {
        outputs.push(underline(homepage));
    }
    outputs.join("\n")
}

// Color helpers. Each is a no-op when stdout is not a terminal (piped or
// captured output), matching chalk's auto-disable so machine-readable
// output stays free of escape codes.
pub(super) fn bright_blue(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_blue()).to_string()
}

pub(super) fn red(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.red()).to_string()
}

fn red_bold(text: &str) -> String {
    let style = owo_colors::Style::new().red().bold();
    text.if_supports_color(Stream::Stdout, |t| t.style(style)).to_string()
}

pub(super) fn green(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.green()).to_string()
}

fn yellow(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.yellow()).to_string()
}

fn grey(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bright_black()).to_string()
}

fn bold(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.bold()).to_string()
}

pub(super) fn dimmed(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.dimmed()).to_string()
}

fn underline(text: &str) -> String {
    text.if_supports_color(Stream::Stdout, |t| t.underline()).to_string()
}
