use super::{
    BlockPlacement,
    DepKind,
    DiffBuckets,
    HashSet,
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
                Some(WorkspaceSummary::new(&self.rendering.cwd, &log.prefix, new_projects));
        }
        self.summary.seen = true;
        self.try_render_summary();
    }

    pub(super) fn try_render_summary(&mut self) {
        if self.summary.rendered {
            return;
        }
        self.apply_manifest_diffs();
        let msg = self.render_summary();
        if msg.is_empty() {
            return;
        }
        self.summary.rendered = true;
        self.notices.deprecations.clear();
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

    /// The dependency sections, followed by the deprecation warnings no
    /// line of theirs carries.
    pub(super) fn render_summary(&self) -> String {
        let mut msg = String::new();
        let mut shown_deprecations = HashSet::new();
        let workspace = self.summary.workspace
            .as_ref()
            .map(|workspace| (workspace, self.new_project_requests(workspace)));
        for kind in SUMMARY_ORDER {
            let lines = match &workspace {
                Some((workspace, requests)) => {
                    self.workspace_summary_lines(workspace, requests, kind)
                }
                None => self.current_project_summary_lines(kind),
            };
            if lines.is_empty() {
                continue;
            }
            shown_deprecations.extend(
                lines
                    .iter()
                    .filter(|(_, diff)| diff.added)
                    .filter_map(|(_, diff)| self.deprecation_of(diff)),
            );
            msg.push('\n');
            msg.push_str(&self.rendering.colors.cyan_bright(&format!("{}:", kind.header())));
            msg.push('\n');
            msg.push_str(
                &lines
                    .into_iter()
                    .map(|(line, _)| line)
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            msg.push('\n');
        }
        let warnings = self.deprecation_warnings(&shown_deprecations);
        if !warnings.is_empty() {
            msg.push('\n');
            msg.push_str(&warnings.join("\n"));
            msg.push('\n');
        }
        msg
    }

    fn current_project_summary_lines(&self, kind: DepKind) -> Vec<(String, &PackageDiff)> {
        let mut diffs: Vec<&PackageDiff> = self.summary.diff[diff_key(kind)]
            .values()
            .filter(|diff| !self.is_hidden_linked(diff))
            .collect();
        diffs.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(u8::from(a.added).cmp(&u8::from(b.added)))
        });
        diffs
            .into_iter()
            .map(|diff| (self.diff_line(diff), diff))
            .collect()
    }

    /// Whether this summary entry is a linked instance of a package the
    /// embedder asked to keep out of the summary. Only linked entries are
    /// hidden: the same package really installed from the registry is a
    /// change worth reporting.
    pub(super) fn is_hidden_linked(&self, pkg: &PackageDiff) -> bool {
        pkg.from.is_some() && self.rendering.hidden_linked_pkgs.matches(&pkg.name)
    }

    pub(super) fn diff_line(&self, pkg: &PackageDiff) -> String {
        let mut result = if pkg.added {
            self.rendering.colors.green("+")
        } else {
            self.rendering.colors.red("-")
        };
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
            result.push_str(&self.rendering.colors.grey(version));
            result.push_str(&self.upgrade_hint(pkg, version));
        }
        if pkg.added {
            result.push_str(&self.deprecation_marker(pkg));
        }
        if let Some(from) = &pkg.from {
            let rel = relative(&self.rendering.cwd, from);
            let shown = if rel.is_empty() { from.clone() } else { rel };
            result.push(' ');
            result.push_str(&self.rendering.colors.grey(&format!("<- {shown}")));
        }
        result
    }

    /// The trailing "is available" note, when the registry's latest is
    /// strictly newer than the installed version.
    ///
    /// A bare `!=` would also fire when the user has pinned a newer version
    /// than the registry's latest tag (e.g. a beta), wrongly suggesting a
    /// downgrade.
    pub(super) fn upgrade_hint(&self, pkg: &PackageDiff, version: &str) -> String {
        let Some(latest) = &pkg.latest else {
            return String::new();
        };
        if latest == version || !is_strictly_newer(latest, version) {
            return String::new();
        }
        format!(" {}", self.rendering.colors.grey(&format!("({latest} is available)")))
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
