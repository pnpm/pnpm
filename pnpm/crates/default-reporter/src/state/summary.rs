use super::{
    BlockPlacement,
    DepKind,
    DiffBuckets,
    HashMap,
    PackageDiff,
    PackageManifestMessage,
    ReporterState,
    SUMMARY_ORDER,
    SummaryLog,
    SummaryScope,
    Value,
    WorkspaceSummary,
    added_diff,
    diff_key,
    empty_diff_buckets,
    is_strictly_newer,
    manifest_dep_versions,
    normalized_prefix,
    record_missing,
    relative,
    remove_optional_from_prod,
    removed_diff,
};
use node_semver::Version;
use pnpm_semver_include_prerelease::{
    VersionChange,
    classify_version_change,
    split_at_changed_field,
};
use std::fmt::Write as _;

impl ReporterState {
    // --- summary ----------------------------------------------------------

    pub(super) fn on_root(&mut self, message: &pnpm_reporter::RootMessage) {
        use pnpm_reporter::RootMessage;
        let prefix = match message {
            RootMessage::Added { prefix, .. } | RootMessage::Removed { prefix, .. } => prefix,
        };
        let buckets = if self.is_current_prefix(prefix) {
            &mut self.summary.diff
        } else {
            self.summary.other_projects
                .entry(normalized_prefix(&self.rendering.cwd, prefix))
                .or_insert_with(empty_diff_buckets)
        };
        let (kind, entry) = match message {
            RootMessage::Added { added, .. } => added_diff(added),
            RootMessage::Removed { removed, .. } => removed_diff(removed),
        };
        record_root_change(buckets, kind, entry);
    }

    pub(super) fn on_manifest(&mut self, message: &PackageManifestMessage) {
        let prefix = match message {
            PackageManifestMessage::Initial { prefix, .. }
            | PackageManifestMessage::Updated { prefix, .. } => prefix,
        };
        let should_render_after_update = matches!(message, PackageManifestMessage::Updated { .. })
            && self.summary.seen;
        {
            let diff = self.summary.manifest_diffs
                .entry(normalized_prefix(&self.rendering.cwd, prefix))
                .or_default();
            match message {
                PackageManifestMessage::Initial { initial, .. } => {
                    diff.initial.get_or_insert_with(|| initial.clone());
                }
                PackageManifestMessage::Updated { updated, .. } => {
                    diff.updated = Some(updated.clone());
                    diff.applied = false;
                }
            }
        }
        if should_render_after_update {
            self.try_render_summary();
        }
    }

    pub(super) fn on_summary(&mut self, log: &SummaryLog) {
        if log.prefix == self.rendering.cwd
            && (self.install.stats_added.is_some() || self.install.stats_removed.is_some())
        {
            self.render_stats();
        }
        if let Some(new_projects) = &log.new_projects
            && self.options.scope.summary == SummaryScope::CurrentPrefix
        {
            self.summary.workspace =
                Some(WorkspaceSummary::new(&self.rendering.cwd, log, new_projects));
        }
        self.summary.seen = true;
        self.try_render_summary();
        self.render_up_to_date();
    }

    pub(super) fn try_render_summary(&mut self) {
        if self.summary.rendered {
            return;
        }
        self.apply_manifest_diffs();
        let msg = self.render_summary_sections();
        if msg.is_empty() {
            return;
        }
        self.summary.rendered = true;
        let mut slot = std::mem::take(&mut self.summary.slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.summary.slot = slot;
    }

    pub(super) fn is_current_prefix(&self, prefix: &str) -> bool {
        self.options.scope.summary == SummaryScope::AllPrefixes
            || prefix == self.rendering.cwd
            || normalized_prefix(&self.rendering.cwd, prefix)
                == normalized_prefix(&self.rendering.cwd, &self.rendering.cwd)
    }

    fn render_summary_sections(&self) -> String {
        let mut msg = String::new();
        let workspace = self.summary.workspace
            .as_ref()
            .map(|workspace| (workspace, self.new_project_requests(workspace)));
        let sections = match &workspace {
            Some((workspace, requests)) => self.workspace_summary_lines(workspace, requests),
            None => SUMMARY_ORDER
                .into_iter()
                .map(|kind| (kind, self.current_project_summary_lines(kind)))
                .collect(),
        };
        for (kind, lines) in sections {
            if lines.is_empty() {
                continue;
            }
            msg.push('\n');
            msg.push_str(&self.rendering.colors.cyan_bright(&format!("{}:", kind.header())));
            msg.push('\n');
            msg.push_str(&lines.join("\n"));
            msg.push('\n');
        }
        msg
    }

    fn current_project_summary_lines(&self, kind: DepKind) -> Vec<String> {
        let mut changes: Vec<Change<'_>> = bucket_changes(&self.summary.diff[diff_key(kind)])
            .into_iter()
            .filter(|change| !self.is_hidden_linked(change.diff))
            .collect();
        changes.sort_by(|a, b| {
            a.diff.name
                .cmp(&b.diff.name)
                .then(u8::from(a.diff.added).cmp(&u8::from(b.diff.added)))
        });
        changes
            .into_iter()
            .map(|change| self.diff_line(&change))
            .collect()
    }

    /// Whether this summary entry is a linked instance of a package the
    /// embedder asked to keep out of the summary. Only linked entries are
    /// hidden: the same package really installed from the registry is a
    /// change worth reporting.
    pub(super) fn is_hidden_linked(&self, pkg: &PackageDiff) -> bool {
        pkg.from.is_some() && self.rendering.hidden_linked_pkgs.matches(&pkg.name)
    }

    pub(super) fn diff_line(&self, change: &Change<'_>) -> String {
        let pkg = change.diff;
        let mut result = self.change_sign(change);
        match &pkg.real_name {
            Some(real) if *real != pkg.name => {
                let _ = write!(result, " {} <- {real}", pkg.name);
            }
            _ => {
                let _ = write!(result, " {}", pkg.name);
            }
        }
        if let Some(version) = &pkg.version {
            result.push(' ');
            result.push_str(&self.version_change(change.replaced_version(), version));
        }
        if let Some(from) = &pkg.from {
            let rel = relative(&self.rendering.cwd, from);
            let shown = if rel.is_empty() { from.clone() } else { rel };
            result.push(' ');
            result.push_str(&self.rendering.colors.grey(&format!("<- {shown}")));
        }
        result
    }

    fn change_sign(&self, change: &Change<'_>) -> String {
        match change.replaced_version() {
            Some(previous) => {
                self.update_sign(previous, change.diff.version.as_deref().unwrap_or_default())
            }
            None if change.diff.added => self.rendering.colors.green("+"),
            None => self.rendering.colors.red("-"),
        }
    }

    /// `version`, after the version it replaced when there is one.
    fn version_change(&self, previous: Option<&str>, version: &str) -> String {
        let colors = &self.rendering.colors;
        match previous {
            Some(previous) => {
                format!(
                    "{}{}",
                    colors.grey(&format!("{previous} → ")),
                    self.version_diff(previous, version),
                )
            }
            None => colors.grey(version),
        }
    }

    /// `version`, with the field bumped from `previous` and the fields
    /// after it colored the way `pnpm outdated` colors them.
    fn version_diff(&self, previous: &str, version: &str) -> String {
        let colors = &self.rendering.colors;
        let change = match (Version::parse(previous), Version::parse(version)) {
            (Ok(previous), Ok(version)) => classify_version_change(&previous, &version),
            _ => VersionChange::Unknown,
        };
        let Some((head, tail)) = split_at_changed_field(version, change) else {
            return colors.grey(version);
        };
        let tail = match change {
            VersionChange::Breaking => colors.red(tail),
            VersionChange::Feature => colors.yellow(tail),
            VersionChange::Fix | VersionChange::None | VersionChange::Unknown => colors.green(tail),
        };
        format!("{}{tail}", colors.grey(head))
    }

    /// `↑` for an upgrade, `↓` for a downgrade, and `~` for a change
    /// between versions that do not compare.
    fn update_sign(&self, previous: &str, version: &str) -> String {
        let sign = if is_strictly_newer(version, previous) {
            "↑"
        } else if is_strictly_newer(previous, version) {
            "↓"
        } else {
            "~"
        };
        self.rendering.colors.yellow(sign)
    }

    /// Fold each manifest edit not folded yet into the changes of its
    /// project. Without a workspace summary only the current project's edits
    /// render.
    fn apply_manifest_diffs(&mut self) {
        let current_project = normalized_prefix(&self.rendering.cwd, &self.rendering.cwd);
        let every_prefix_is_current = self.options.scope.summary == SummaryScope::AllPrefixes;
        let summary = &mut self.summary;
        for (prefix, edit) in &mut summary.manifest_diffs {
            let (false, Some(initial), Some(updated)) =
                (edit.applied, &edit.initial, &edit.updated)
            else {
                continue;
            };
            let buckets = if every_prefix_is_current || *prefix == current_project {
                &mut summary.diff
            } else if summary.workspace.is_some() {
                summary.other_projects.entry(prefix.clone()).or_insert_with(empty_diff_buckets)
            } else {
                continue;
            };
            apply_manifest_pair_diff(buckets, initial, updated);
            edit.applied = true;
        }
    }
}

/// One summary line: an addition, a removal, or an addition that
/// replaced a removed version of the same dependency.
pub(super) struct Change<'a> {
    /// The added side of an update.
    pub(super) diff: &'a PackageDiff,
    replaced: Option<&'a PackageDiff>,
}

impl<'a> Change<'a> {
    pub(super) fn replaced_version(&self) -> Option<&'a str> {
        self.replaced.and_then(|replaced| replaced.version.as_deref())
    }
}

/// The changes of one bucket. A removal and an addition of the same
/// dependency, both of a registry version, fold into one update.
pub(super) fn bucket_changes(bucket: &HashMap<String, PackageDiff>) -> Vec<Change<'_>> {
    let counterpart = |diff: &PackageDiff| {
        let sign = if diff.added { '-' } else { '+' };
        bucket
            .get(&format!("{sign}{}", diff.name))
            .filter(|other| is_versioned_registry_dep(diff) && is_versioned_registry_dep(other))
    };
    bucket
        .values()
        .filter_map(|diff| match (diff.added, counterpart(diff)) {
            (false, Some(_)) => None,
            (true, replaced) => Some(Change { diff, replaced }),
            (false, None) => Some(Change { diff, replaced: None }),
        })
        .collect()
}

fn is_versioned_registry_dep(diff: &PackageDiff) -> bool {
    diff.version.is_some() && diff.from.is_none()
}

/// Record one `pnpm:root` change. A change that undoes an earlier one of
/// the same version cancels it.
fn record_root_change(buckets: &mut DiffBuckets, kind: DepKind, entry: PackageDiff) {
    let Some(bucket) = buckets.get_mut(diff_key(kind)) else {
        return;
    };
    let opposite_key = format!("{}{}", if entry.added { '-' } else { '+' }, entry.name);
    if let Some(prev) = bucket.get(&opposite_key)
        && prev.version == entry.version
    {
        bucket.remove(&opposite_key);
        return;
    }
    let entry_key = format!("{}{}", if entry.added { '+' } else { '-' }, entry.name);
    bucket.insert(entry_key, entry);
}

fn apply_manifest_pair_diff(buckets: &mut DiffBuckets, initial: &Value, updated: &Value) {
    let initial = remove_optional_from_prod(initial);
    let updated = remove_optional_from_prod(updated);
    for kind in [DepKind::Peer, DepKind::Prod, DepKind::Optional, DepKind::Dev] {
        let prop = kind.header();
        let initial_deps = manifest_dep_versions(&initial, prop);
        let updated_deps = manifest_dep_versions(&updated, prop);
        if let Some(bucket) = buckets.get_mut(diff_key(kind)) {
            record_missing(bucket, &initial_deps, &updated_deps, false);
            record_missing(bucket, &updated_deps, &initial_deps, true);
        }
    }
}
