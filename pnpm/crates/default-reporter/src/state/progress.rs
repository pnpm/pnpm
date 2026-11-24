use super::{
    BigTarball,
    BlockPlacement,
    BlockSlot,
    ContextLog,
    DownloadState,
    FetchingProgressMessage,
    Frame,
    InstallProgress,
    PackageImportMethod,
    ProgressMessage,
    ProgressOptions,
    RenderingContext,
    ReporterState,
    ScopeLog,
    Stage,
    StatsMessage,
    normalize,
    pretty_bytes,
    relative,
    zoom_out,
};

use std::fmt::Write as _;

impl ReporterState {
    // --- scope ------------------------------------------------------------

    /// pnpm's `reportScope`: how many workspace projects the command
    /// selected. Silent for a command that doesn't report scope, for a
    /// single selected project, where the answer is the directory the
    /// user is already standing in, and when every project was selected,
    /// which the user did not have to ask for.
    pub(super) fn on_scope(&mut self, log: &ScopeLog) {
        if !self.options.scope.reports_scope || log.selected == 1 {
            return;
        }
        let count = match log.total {
            Some(total) if total == log.selected => return,
            Some(total) => format!("{} of {total}", log.selected),
            None => log.selected.to_string(),
        };
        let unit = if log.workspace_prefix.is_some() { "workspace projects" } else { "projects" };
        let mut slot = std::mem::take(&mut self.display.scope_slot);
        self.display.frame.emit(
            &mut slot,
            format!("Scope: {count} {unit}"),
            BlockPlacement::Scrolling,
        );
        self.display.scope_slot = slot;
    }

    // --- context ----------------------------------------------------------

    pub(super) fn on_context(&mut self, log: &ContextLog) {
        self.install.context = Some(log.clone());
        self.install.maybe_render_context(&self.rendering.cwd, &mut self.display.frame);
    }

    // --- progress ---------------------------------------------------------

    pub(super) fn on_progress(&mut self, message: &ProgressMessage) {
        let requester = match message {
            ProgressMessage::Resolved { requester, .. }
            | ProgressMessage::Fetched { requester, .. }
            | ProgressMessage::FoundInStore { requester, .. }
            | ProgressMessage::Imported { requester, .. } => requester.clone(),
        };
        let entry = self.downloads.progress.entry(requester.clone()).or_default();
        // A stats line counted this requester's last install, so this is
        // a later install of it.
        if entry.done && entry.reported {
            *entry = super::ProgressEntry::default();
        }
        if entry.done {
            return;
        }
        match message {
            ProgressMessage::Resolved { .. } => entry.stats.resolved += 1,
            ProgressMessage::Fetched { .. } => entry.stats.fetched += 1,
            ProgressMessage::FoundInStore { .. } => entry.stats.reused += 1,
            ProgressMessage::Imported { .. } => entry.stats.imported += 1,
        }
        let msg = self.downloads.progress_message(
            &self.rendering,
            &self.options.progress,
            &requester,
            false,
        );
        let Some(entry) = self.downloads.progress.get_mut(&requester) else { return };
        let mut slot = std::mem::take(&mut entry.slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Pinned);
        if let Some(entry) = self.downloads.progress.get_mut(&requester) {
            entry.slot = slot;
        }
    }

    pub(super) fn on_stage(&mut self, prefix: &str, stage: Stage) {
        match stage {
            Stage::ResolutionDone => self.flush_unused_overrides(),
            Stage::ImportingDone => {
                let Some(entry) = self.downloads.progress.get_mut(prefix) else { return };
                if entry.reported {
                    entry.done = true;
                    self.display.frame.clear(&mut entry.slot);
                    return;
                }
                let msg = self.downloads.progress_message(
                    &self.rendering,
                    &self.options.progress,
                    prefix,
                    true,
                );
                let Some(entry) = self.downloads.progress.get_mut(prefix) else { return };
                let mut slot = std::mem::take(&mut entry.slot);
                self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
                if let Some(entry) = self.downloads.progress.get_mut(prefix) {
                    entry.slot = slot;
                    entry.done = true;
                }
                if self.downloads.importing_finished() {
                    self.render_stats();
                }
            }
            _ => {}
        }
    }

    // --- big tarballs -----------------------------------------------------

    pub(super) fn on_fetching(&mut self, message: &FetchingProgressMessage) {
        match message {
            FetchingProgressMessage::Started { attempt, package_id, size } => {
                self.start_tarball_block(*attempt, package_id, *size);
            }
            FetchingProgressMessage::InProgress { downloaded, package_id } => {
                self.rewrite_tarball_block(package_id, *downloaded);
            }
        }
    }

    /// Give a big tarball a block of its own, on the first attempt only, so
    /// a retry does not open a second block for the same download.
    fn start_tarball_block(&mut self, attempt: u32, package_id: &str, size: Option<u64>) {
        const BIG_TARBALL_SIZE: u64 = 1024 * 1024 * 5;
        let Some(size) = size else { return };
        if size < BIG_TARBALL_SIZE || attempt != 1 {
            return;
        }
        let mut entry = BigTarball { size, slot: BlockSlot::default() };
        let msg = self.downloading_message(package_id, 0, size);
        self.display.frame.emit(&mut entry.slot, msg, BlockPlacement::Pinned);
        self.downloads.tarballs.insert(package_id.to_string(), entry);
    }

    fn rewrite_tarball_block(&mut self, package_id: &str, downloaded: u64) {
        let Some(entry) = self.downloads.tarballs.get(package_id) else { return };
        let size = entry.size;
        let msg = self.downloading_message(package_id, downloaded, size);
        // A finished download stops being rewritten in place and scrolls
        // away with the rest of the output.
        let placement =
            if downloaded == size { BlockPlacement::Scrolling } else { BlockPlacement::Pinned };
        let Some(entry) = self.downloads.tarballs.get_mut(package_id) else { return };
        let mut slot = std::mem::take(&mut entry.slot);
        self.display.frame.emit(&mut slot, msg, placement);
        if let Some(entry) = self.downloads.tarballs.get_mut(package_id) {
            entry.slot = slot;
        }
    }

    pub(super) fn downloading_message(
        &self,
        package_id: &str,
        downloaded: u64,
        size: u64,
    ) -> String {
        let done = downloaded == size;
        let suffix = if done { ", done" } else { "" };
        format!(
            "Downloading {package_id}: {}/{}{suffix}",
            self.rendering.colors.cyan_bright(&pretty_bytes(downloaded)),
            self.rendering.colors.cyan_bright(&pretty_bytes(size)),
        )
    }

    // --- stats ------------------------------------------------------------

    pub(super) fn on_stats(&mut self, message: &StatsMessage) {
        let prefix = match message {
            StatsMessage::Added { prefix, .. } | StatsMessage::Removed { prefix, .. } => prefix,
        };
        // Run from a workspace member, an install reports both its progress
        // and its stats under the workspace root.
        if prefix != &self.rendering.cwd && !self.downloads.progress.contains_key(prefix) {
            return;
        }
        let stats = self.install.stats.entry(prefix.clone()).or_default();
        match message {
            StatsMessage::Added { added, .. } => stats.added = Some(*added),
            StatsMessage::Removed { removed, .. } => stats.removed = Some(*removed),
        }
        if self.downloads.importing_finished() {
            self.render_stats();
        }
    }

    /// "Already up to date", when neither the packages nor the summary
    /// changed.
    pub(super) fn render_up_to_date(&mut self) {
        if !std::mem::take(&mut self.install.no_package_changed) || self.summary.rendered {
            return;
        }
        let mut slot = std::mem::take(&mut self.install.stats_slot);
        self.display.frame.emit(
            &mut slot,
            "Already up to date".to_string(),
            BlockPlacement::Scrolling,
        );
        self.install.stats_slot = slot;
    }

    /// The stats that replace the finished progress lines: a line such as
    /// `Packages: +5 -2 (60% downloaded)`, and a bar with a sign per
    /// package: `↓` downloaded, `+` reused from the store, `-` removed.
    /// The line gives the larger share of downloaded and reused packages,
    /// not their counts: resolution also fetches versions it then drops,
    /// so a count would not match the packages added.
    ///
    /// Waits for both halves of every project's stats. Each line consumes
    /// the stats and the progress it counts, so a project or an install
    /// that finishes later gets a line of its own.
    pub(super) fn render_stats(&mut self) {
        let Some((added, removed)) = self.install.stats_totals() else { return };
        let requesters: Vec<String> = self.install.stats
            .keys()
            .cloned()
            .collect();
        let (downloaded, reused) = self.downloads.package_sources(&requesters);
        let counts = StatsCounts { added, removed, downloaded, reused };
        let changed = added > 0 || removed > 0;
        let msg = changed.then(|| self.stats_message(counts));
        self.downloads.mark_reported(&requesters, &mut self.display.frame);
        self.install.stats.clear();
        self.install.package_changed |= changed;
        self.install.no_package_changed = !self.install.package_changed;
        let Some(msg) = msg else { return };
        let mut slot = std::mem::take(&mut self.install.stats_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
    }

    fn stats_message(&self, counts: StatsCounts) -> String {
        let StatsCounts { added, removed, .. } = counts;
        let colors = &self.rendering.colors;
        let mut msg = String::from("Packages:");
        if added > 0 {
            msg.push(' ');
            msg.push_str(&colors.green(&format!("+{added}")));
        }
        if removed > 0 {
            msg.push(' ');
            msg.push_str(&colors.red(&format!("-{removed}")));
        }
        if let Some(source) = counts.source_share() {
            let _ = write!(msg, " ({source})");
        }
        msg.push('\n');
        msg.push_str(&self.pluses_and_minuses(self.rendering.width, counts));
        msg
    }

    fn pluses_and_minuses(&self, max_width: usize, counts: StatsCounts) -> String {
        let StatsCounts { added, removed, .. } = counts;
        if max_width == 0 {
            return String::new();
        }
        let changes = added + removed;
        let (added_chars, removed_chars) = if changes > max_width as u64 {
            if added == 0 {
                (0, max_width)
            } else if removed == 0 {
                (max_width, 0)
            } else {
                let ratio = max_width as f64 / changes as f64;
                let added_chars = ((added as f64 * ratio).floor() as usize)
                    .max(1)
                    .min(max_width.saturating_sub(1));
                (added_chars, max_width - added_chars)
            }
        } else {
            (added as usize, removed as usize)
        };
        let downloaded_chars = counts.downloaded_part(added_chars);
        let mut out = String::new();
        for _ in 0..downloaded_chars {
            out.push_str(&self.rendering.colors.green("↓"));
        }
        for _ in downloaded_chars..added_chars {
            out.push_str(&self.rendering.colors.green("+"));
        }
        for _ in 0..removed_chars {
            out.push_str(&self.rendering.colors.red("-"));
        }
        out
    }
}

impl DownloadState {
    /// Whether every project's progress line has finished, or a stats
    /// line already counted it. True when there was none. A Python install
    /// reports progress too, but never finishes importing, and its packages
    /// are not in the stats.
    pub(super) fn importing_finished(&self) -> bool {
        self.npm_progress()
            .all(|entry| entry.done || entry.reported)
    }

    fn npm_progress(&self) -> impl Iterator<Item = &super::ProgressEntry> {
        self.progress
            .iter()
            .filter(|(requester, _)| *requester != pnpm_reporter::PYTHON_ENVIRONMENT_REQUESTER)
            .map(|(_, entry)| entry)
    }

    /// How many packages `requesters` downloaded, and how many they reused
    /// from the store, leaving out what an earlier stats line counted.
    fn package_sources(&self, requesters: &[String]) -> (u64, u64) {
        requesters
            .iter()
            .filter_map(|requester| self.progress.get(requester))
            .filter(|entry| !entry.reported)
            .fold((0, 0), |(downloaded, reused), entry| {
                (downloaded + entry.stats.fetched, reused + entry.stats.reused)
            })
    }

    pub(super) fn progress_message(
        &self,
        rendering: &RenderingContext,
        options: &ProgressOptions,
        requester: &str,
        done: bool,
    ) -> String {
        let stats = self.progress
            .get(requester)
            .map(|entry| entry.stats)
            .unwrap_or_default();
        let hl = |count: u64| rendering.colors.cyan_bright(&count.to_string());
        let mut msg = format!(
            "Progress: resolved {}, reused {}, downloaded {}",
            hl(stats.resolved),
            hl(stats.reused),
            hl(stats.fetched),
        );
        if !options.hide_added_pkgs {
            msg.push_str(", added ");
            msg.push_str(&hl(stats.imported));
        }
        if done {
            msg.push_str(", done");
        }
        if !options.hide_prefix && requester != rendering.cwd {
            msg = zoom_out(&rendering.cwd, requester, &msg);
        }
        msg
    }

    /// Marks the progress of `requesters`, whose stats a stats line
    /// counts, and clears the progress lines of those that finished, which
    /// the stats line replaces. A project that has not finished yet loses
    /// its line when it does.
    fn mark_reported(&mut self, requesters: &[String], frame: &mut Frame) {
        for requester in requesters {
            let Some(entry) = self.progress.get_mut(requester) else { continue };
            if entry.reported {
                continue;
            }
            entry.reported = true;
            if entry.done {
                frame.clear(&mut entry.slot);
            }
        }
    }
}

/// What a stats line shows: the added and removed totals, and the
/// downloaded and reused counts of the same projects.
#[derive(Clone, Copy)]
struct StatsCounts {
    added: u64,
    removed: u64,
    downloaded: u64,
    reused: u64,
}

impl StatsCounts {
    /// Where most packages came from, as `60% downloaded` or `90% reused`.
    /// Rounded down, so 100% means every package. `None` when no package
    /// was downloaded or reused.
    fn source_share(self) -> Option<String> {
        let total = self.downloaded + self.reused;
        if total == 0 {
            return None;
        }
        let (share, source) = if self.downloaded >= self.reused {
            (self.downloaded, "downloaded")
        } else {
            (self.reused, "reused")
        };
        Some(format!("{}% {source}", share * 100 / total))
    }

    /// How many of the bar's `chars` added signs are downloaded `↓`
    /// packages, in the share the line reports. A download keeps at
    /// least one sign of its own. Zero when the sources are unknown.
    fn downloaded_part(self, chars: usize) -> usize {
        let total = self.downloaded + self.reused;
        if total == 0 {
            return 0;
        }
        let part = usize::try_from(self.downloaded * chars as u64 / total).unwrap_or(chars);
        if self.downloaded > 0 { part.max(1).min(chars) } else { part }
    }
}

impl InstallProgress {
    /// The stats summed over the projects, once every project has both
    /// halves. `None` before any stats arrive.
    fn stats_totals(&self) -> Option<(u64, u64)> {
        if self.stats.is_empty() {
            return None;
        }
        self.stats
            .values()
            .try_fold((0, 0), |(added, removed), stats| {
                Some((added + stats.added?, removed + stats.removed?))
            })
    }

    /// Counts a half that never arrived as zero, so the stats can render
    /// when the install ends without it.
    pub(super) fn complete_stats(&mut self) {
        for stats in self.stats.values_mut() {
            stats.added.get_or_insert(0);
            stats.removed.get_or_insert(0);
        }
    }

    pub(super) fn maybe_render_context(&mut self, cwd: &str, frame: &mut Frame) {
        if self.context_rendered {
            return;
        }
        let (Some(ctx), Some(method)) = (self.context.as_ref(), self.import_method) else {
            return;
        };
        if ctx.current_lockfile_exists {
            self.context_rendered = true;
            return;
        }
        let method = match method {
            PackageImportMethod::Copy => "copied",
            PackageImportMethod::Clone => "cloned",
            PackageImportMethod::Hardlink => "hard linked",
        };
        let virtual_store = normalize(&relative(cwd, &ctx.virtual_store_dir));
        let msg = format!(
            "Packages are {method} from the content-addressable store to the virtual store.\n  \
             Content-addressable store is at: {}\n  Virtual store is at:             {}",
            ctx.store_dir, virtual_store,
        );
        self.context_rendered = true;
        let mut slot = std::mem::take(&mut self.context_slot);
        frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.context_slot = slot;
    }
}
