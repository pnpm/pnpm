use super::{
    DepKind, PackageDiff, PackageManifestMessage, ReporterState, SUMMARY_ORDER, SummaryScope,
    Value, added_diff, diff_key, is_strictly_newer, manifest_dep_versions, normalized_prefix,
    record_missing, relative, remove_optional_from_prod, removed_diff,
};
use std::fmt::Write as _;

impl ReporterState {
    // --- summary ----------------------------------------------------------

    pub(super) fn on_root(&mut self, message: &pnpm_reporter::RootMessage) {
        use pnpm_reporter::RootMessage;
        let prefix = match message {
            RootMessage::Added { prefix, .. } | RootMessage::Removed { prefix, .. } => prefix,
        };
        if !self.is_current_prefix(prefix) {
            return;
        }
        let (kind, entry) = match message {
            RootMessage::Added { added, .. } => added_diff(added),
            RootMessage::Removed { removed, .. } => removed_diff(removed),
        };
        let key = diff_key(kind);
        let opposite_key = format!("{}{}", if entry.added { '-' } else { '+' }, entry.name);
        if let Some(prev) = self.diff.get(key).and_then(|b| b.get(&opposite_key))
            && prev.version == entry.version
        {
            self.diff.get_mut(key).unwrap().remove(&opposite_key);
            return;
        }
        self.diff
            .get_mut(key)
            .unwrap()
            .insert(format!("{}{}", if entry.added { '+' } else { '-' }, entry.name), entry);
    }

    pub(super) fn on_manifest(&mut self, message: &PackageManifestMessage) {
        let prefix = match message {
            PackageManifestMessage::Initial { prefix, .. }
            | PackageManifestMessage::Updated { prefix, .. } => prefix,
        };
        if !self.is_current_prefix(prefix) {
            return;
        }
        let should_render_after_update =
            matches!(message, PackageManifestMessage::Updated { .. }) && self.summary_seen;
        {
            let diff = self.manifest_diffs.entry(prefix.clone()).or_default();
            match message {
                PackageManifestMessage::Initial { initial, .. } => {
                    diff.initial.get_or_insert_with(|| initial.clone());
                }
                PackageManifestMessage::Updated { updated, .. } => {
                    diff.updated = Some(updated.clone());
                }
            }
        }
        if should_render_after_update {
            self.try_render_summary();
        }
    }

    pub(super) fn on_summary(&mut self, prefix: &str) {
        if prefix == self.cwd && (self.stats_added.is_some() || self.stats_removed.is_some()) {
            self.render_stats();
        }
        self.summary_seen = true;
        self.try_render_summary();
    }

    pub(super) fn try_render_summary(&mut self) {
        if self.summary_rendered {
            return;
        }
        self.apply_manifest_diff();
        let msg = self.render_summary();
        if msg.is_empty() {
            return;
        }
        self.summary_rendered = true;
        let mut slot = std::mem::take(&mut self.summary_slot);
        self.frame.emit(&mut slot, msg, false);
        self.summary_slot = slot;
    }

    pub(super) fn is_current_prefix(&self, prefix: &str) -> bool {
        self.options.summary_scope == SummaryScope::AllPrefixes
            || prefix == self.cwd
            || normalized_prefix(&self.cwd, prefix) == normalized_prefix(&self.cwd, &self.cwd)
    }

    pub(super) fn apply_manifest_diff(&mut self) {
        let manifest_diffs: Vec<(Value, Value)> = self
            .manifest_diffs
            .values()
            .filter_map(|diff| {
                Some((diff.initial.as_ref()?.clone(), diff.updated.as_ref()?.clone()))
            })
            .collect();
        for (initial, updated) in manifest_diffs {
            self.apply_manifest_pair_diff(&initial, &updated);
        }
    }

    pub(super) fn apply_manifest_pair_diff(&mut self, initial: &Value, updated: &Value) {
        let initial = remove_optional_from_prod(initial);
        let updated = remove_optional_from_prod(updated);
        for kind in [DepKind::Peer, DepKind::Prod, DepKind::Optional, DepKind::Dev] {
            let prop = kind.header();
            let initial_deps = manifest_dep_versions(&initial, prop);
            let updated_deps = manifest_dep_versions(&updated, prop);
            let bucket = self.diff.get_mut(diff_key(kind)).unwrap();
            record_missing(bucket, &initial_deps, &updated_deps, false);
            record_missing(bucket, &updated_deps, &initial_deps, true);
        }
    }

    pub(super) fn render_summary(&self) -> String {
        let mut msg = String::new();
        for kind in SUMMARY_ORDER {
            let bucket = &self.diff[diff_key(kind)];
            if bucket.is_empty() {
                continue;
            }
            let mut diffs: Vec<&PackageDiff> =
                bucket.values().filter(|diff| !self.is_hidden_linked(diff)).collect();
            if diffs.is_empty() {
                continue;
            }
            diffs.sort_by(|a, b| {
                a.name.cmp(&b.name).then(u8::from(a.added).cmp(&u8::from(b.added)))
            });
            msg.push('\n');
            msg.push_str(&self.colors.cyan_bright(&format!("{}:", kind.header())));
            msg.push('\n');
            let lines: Vec<String> = diffs.iter().map(|diff| self.diff_line(diff)).collect();
            msg.push_str(&lines.join("\n"));
            msg.push('\n');
        }
        msg
    }

    /// Whether this summary entry is a linked instance of a package the
    /// embedder asked to keep out of the summary. Only linked entries are
    /// hidden: the same package really installed from the registry is a
    /// change worth reporting.
    pub(super) fn is_hidden_linked(&self, pkg: &PackageDiff) -> bool {
        pkg.from.is_some() && self.hidden_linked_pkgs.matches(&pkg.name)
    }

    pub(super) fn diff_line(&self, pkg: &PackageDiff) -> String {
        let mut result = if pkg.added { self.colors.green("+") } else { self.colors.red("-") };
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
            result.push_str(&self.colors.grey(version));
            result.push_str(&self.upgrade_hint(pkg, version));
        }
        if let Some(from) = &pkg.from {
            let rel = relative(&self.cwd, from);
            let shown = if rel.is_empty() { from.clone() } else { rel };
            result.push(' ');
            result.push_str(&self.colors.grey(&format!("<- {shown}")));
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
        format!(" {}", self.colors.grey(&format!("({latest} is available)")))
    }
}
