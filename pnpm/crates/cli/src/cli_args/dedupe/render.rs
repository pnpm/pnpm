use super::{
    Lockfile, LockfileDiff, Map, Path, SnapshotDiff, TreeNode, Value, blue_bright_underline, gray,
    green, json, plain, red, render_archy,
};

/// Parse one side of the `--check` diff. A snapshot that does not parse —
/// an older lockfile format the dedupe install has just rewritten, say —
/// yields no baseline rather than replacing the check's verdict with a
/// parse error: the run already knows the lockfile would change, and only
/// the detail of the report is lost.
pub(super) fn parse_snapshot(content: Option<&str>, lockfile_path: &Path) -> Option<Lockfile> {
    content.and_then(|content| Lockfile::parse(content, lockfile_path).ok().flatten())
}

/// Render what `pnpm dedupe` would rewrite, mirroring pnpm's
/// `renderDedupeCheckIssues`: one tree per changed importer or package
/// snapshot, plus the snapshots deduplication would add or drop.
///
/// The lockfile can also be rewritten without any resolution changing —
/// recorded settings drift, a config dependency the run synced — so an
/// empty diff still says why the check failed.
pub(super) fn render_dedupe_check_issues(diff: &LockfileDiff) -> String {
    if diff.is_empty() {
        return "The lockfile would be rewritten, but no dependency resolution would change."
            .to_string();
    }
    [
        render_section("Importers", &diff.importers, &[], &[]),
        render_section(
            "Packages",
            &diff.updated_packages,
            &diff.added_packages,
            &diff.removed_packages,
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n")
}

pub(super) fn render_dedupe_check_error(diff: &LockfileDiff) -> String {
    let issues = render_dedupe_check_issues(diff);
    let recommendation_separator = if issues.ends_with("\n\n") {
        ""
    } else if issues.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!(
        "[ERR_PNPM_DEDUPE_CHECK_ISSUES] Dedupe --check found changes to the lockfile\n\n{issues}{recommendation_separator}Run pnpm dedupe to apply the changes above.\n",
    )
}

pub(super) fn dedupe_check_issues_json(diff: &LockfileDiff) -> Value {
    json!({
        "importerIssuesByImporterId": snapshots_changes_json(&diff.importers, &[], &[]),
        "packageIssuesByDepPath": snapshots_changes_json(
            &diff.updated_packages,
            &diff.added_packages,
            &diff.removed_packages,
        ),
    })
}

pub(super) fn snapshots_changes_json(
    updated: &[SnapshotDiff],
    added: &[String],
    removed: &[String],
) -> Value {
    let updated = updated
        .iter()
        .map(|snapshot| {
            let changes = snapshot.added
                .iter()
                .map(|(alias, next)| (alias.clone(), json!({ "type": "added", "next": next })))
                .chain(
                    snapshot.removed
                        .iter()
                        .map(|(alias, prev)| {
                            (alias.clone(), json!({ "type": "removed", "prev": prev }))
                        }),
                )
                .chain(
                    snapshot.updated
                        .iter()
                        .map(|(alias, prev, next)| {
                            (
                                alias.clone(),
                                json!({ "type": "updated", "prev": prev, "next": next }),
                            )
                        }),
                )
                .collect::<Map<_, _>>();
            (snapshot.id.clone(), Value::Object(changes))
        })
        .collect::<Map<_, _>>();
    json!({
        "added": added,
        "removed": removed,
        "updated": updated,
    })
}

pub(super) fn render_section(
    title: &str,
    updated: &[SnapshotDiff],
    added: &[String],
    removed: &[String],
) -> Option<String> {
    let mut lines: Vec<String> = updated
        .iter()
        .map(render_snapshot_diff)
        .collect();
    lines.extend(
        added
            .iter()
            .map(|id| format!("{} {}", green("+"), plain(id))),
    );
    lines.extend(
        removed
            .iter()
            .map(|id| format!("{} {}", red("-"), plain(id))),
    );
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "{}\n{}\n",
        blue_bright_underline(title),
        lines.join("\n"),
    ))
}

pub(super) fn render_snapshot_diff(diff: &SnapshotDiff) -> String {
    let added = diff.added
        .iter()
        .map(|(alias, next)| format!("{} {} {}", green("+"), plain(alias), gray(next)));
    let removed = diff.removed
        .iter()
        .map(|(alias, prev)| format!("{} {} {}", red("-"), plain(alias), gray(prev)));
    let updated = diff.updated
        .iter()
        .map(|(alias, prev, next)| {
            format!(
                "{} {} {} {}",
                plain(alias),
                red(prev),
                gray("→"),
                green(next),
            )
        });
    let nodes = added
        .chain(removed)
        .chain(updated)
        .map(|label| TreeNode::with_children(label, Vec::new()))
        .collect();
    render_archy(&TreeNode::with_children(plain(&diff.id), nodes))
}
