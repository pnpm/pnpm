use super::{pending_permissions, short_package_label};
use pnpm_config::{Config, SkillsPermission};
use std::{collections::BTreeMap, fmt::Write as _};

pub(crate) fn render_permissions(config: &Config) -> miette::Result<String> {
    let mut granted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut denied: BTreeMap<String, Vec<String>> = BTreeMap::new();
    populate_build_permissions(&config.allow_builds, &mut granted, &mut denied);
    populate_skills_permissions(&config.allow_skills, &mut granted, &mut denied);

    let pending = pending_permissions_section(config)?;
    let mut output = String::new();
    for (title, section) in
        [("Granted", &granted), ("Denied", &denied), ("Awaiting approval", &pending)]
    {
        render_section(&mut output, title, section);
    }
    if output.is_empty() {
        output.push_str("No packages have been approved, denied, or are awaiting approval\n");
    } else if !pending.is_empty() {
        output.push_str("\nRun \"pnpm approve\" to review the packages awaiting approval.\n");
    }
    Ok(output)
}

fn populate_build_permissions(
    allow_builds: &std::collections::HashMap<String, bool>,
    granted: &mut BTreeMap<String, Vec<String>>,
    denied: &mut BTreeMap<String, Vec<String>>,
) {
    for (pkg, &allowed) in allow_builds {
        let section = if allowed { &mut *granted } else { &mut *denied };
        section
            .entry(short_package_label(pkg))
            .or_default()
            .push("build".to_string());
    }
}

fn populate_skills_permissions(
    allow_skills: &std::collections::HashMap<String, SkillsPermission>,
    granted: &mut BTreeMap<String, Vec<String>>,
    denied: &mut BTreeMap<String, Vec<String>>,
) {
    for (pkg, decision) in allow_skills {
        let label = match decision {
            SkillsPermission::All(_) => "skills".to_string(),
            SkillsPermission::Chosen(chosen) => format!("skills ({})", chosen.join(", ")),
        };
        let section = if decision.links_any() { &mut *granted } else { &mut *denied };
        section
            .entry(short_package_label(pkg))
            .or_default()
            .push(label);
    }
}

fn pending_permissions_section(config: &Config) -> miette::Result<BTreeMap<String, Vec<String>>> {
    let pending = pending_permissions(config)?.by_package;
    Ok(pending
        .into_iter()
        .map(|(pkg, capabilities)| {
            (
                short_package_label(&pkg),
                capabilities
                    .into_iter()
                    .map(|capability| capability.as_str().to_string())
                    .collect(),
            )
        })
        .collect())
}

fn render_section(output: &mut String, title: &str, section: &BTreeMap<String, Vec<String>>) {
    if section.is_empty() {
        return;
    }
    if !output.is_empty() {
        output.push('\n');
    }
    let width = section
        .keys()
        .map(String::len)
        .max()
        .unwrap_or_default();
    output.push_str(title);
    output.push_str(":\n");
    for (pkg, labels) in section {
        writeln!(output, "  {pkg:width$}  {}", labels.join(", ")).expect("write to a String");
    }
}
