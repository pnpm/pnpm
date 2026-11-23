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
            ProgressLine::Live,
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
            // In a terminal the live progress line gives way to the one
            // line of stats. Append-only output throttles progress lines
            // and cannot clear them, so it ends with the final counts.
            Stage::ImportingDone => {
                if !self.downloads.progress.contains_key(prefix) {
                    return;
                }
                let final_counts = self.options.append_only.then(|| {
                    self.downloads.progress_message(
                        &self.rendering,
                        &self.options.progress,
                        prefix,
                        ProgressLine::Done,
                    )
                });
                let Some(entry) = self.downloads.progress.get_mut(prefix) else { return };
                match final_counts {
                    Some(msg) => {
                        self.display.frame.emit(&mut entry.slot, msg, BlockPlacement::Scrolling);
                    }
                    None => {
                        self.display.frame.clear(&mut entry.slot);
                    }
                }
                entry.done = true;
                if self.install.stats_added.is_some()
                    && self.install.stats_removed.is_some()
                {
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
        // Append-only output cannot rewrite the line as the download
        // advances, so it would only print where the download started.
        if size < BIG_TARBALL_SIZE || attempt != 1 || self.options.append_only {
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
        if downloaded == size {
            if let Some(mut entry) = self.downloads.tarballs.remove(package_id) {
                self.display.frame.clear(&mut entry.slot);
            }
            return;
        }
        let msg = self.downloading_message(package_id, downloaded, size);
        let Some(entry) = self.downloads.tarballs.get_mut(package_id) else { return };
        let mut slot = std::mem::take(&mut entry.slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Pinned);
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
        format!(
            "Downloading {package_id}: {}/{}",
            self.rendering.colors.cyan_bright(&pretty_bytes(downloaded)),
            self.rendering.colors.cyan_bright(&pretty_bytes(size)),
        )
    }

    // --- stats ------------------------------------------------------------

    pub(super) fn on_stats(&mut self, message: &StatsMessage) {
        let prefix = match message {
            StatsMessage::Added { prefix, .. } | StatsMessage::Removed { prefix, .. } => prefix,
        };
        if prefix != &self.rendering.cwd {
            return;
        }
        match message {
            StatsMessage::Added { added, .. } => {
                self.install.stats_added = Some(*added);
            }
            StatsMessage::Removed { removed, .. } => {
                self.install.stats_removed = Some(*removed);
            }
        }
        if self.install.stats_added.is_some()
            && self.install.stats_removed.is_some()
            && self.downloads.importing_finished()
        {
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

    /// The one line an install's progress ends with, such as
    /// `Packages: +5 -2 (60% downloaded)`. It gives the larger share of
    /// downloaded and reused packages, not their counts: resolution also
    /// fetches versions it then drops, so a count would not match the
    /// packages added.
    pub(super) fn render_stats(&mut self) {
        let added = self.install.stats_added.take().unwrap_or(0);
        let removed = self.install.stats_removed.take().unwrap_or(0);
        if added == 0 && removed == 0 {
            self.install.no_package_changed = true;
            return;
        }
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
        if let Some(source) = self.downloads.package_source() {
            let _ = write!(msg, " ({source})");
        }
        let mut slot = std::mem::take(&mut self.install.stats_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.install.stats_slot = slot;
    }
}

/// Whether a progress line is still updating or reports the final counts.
#[derive(Clone, Copy)]
pub(super) enum ProgressLine {
    Live,
    Done,
}

impl DownloadState {
    /// Whether every live progress line has finished. True when there
    /// was none.
    pub(super) fn importing_finished(&self) -> bool {
        self.progress.values().all(|entry| entry.done)
    }

    /// Where most packages came from, as `60% downloaded` or `90% reused`.
    /// Rounded down, so 100% means every package. `None` when no package
    /// was downloaded or reused.
    fn package_source(&self) -> Option<String> {
        let (downloaded, reused) = self.progress
            .values()
            .fold((0, 0), |(downloaded, reused), entry| {
                (downloaded + entry.stats.fetched, reused + entry.stats.reused)
            });
        let total = downloaded + reused;
        if total == 0 {
            return None;
        }
        let (share, source) =
            if downloaded >= reused { (downloaded, "downloaded") } else { (reused, "reused") };
        Some(format!("{}% {source}", share * 100 / total))
    }

    pub(super) fn progress_message(
        &self,
        rendering: &RenderingContext,
        options: &ProgressOptions,
        requester: &str,
        line: ProgressLine,
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
        match line {
            ProgressLine::Live => {}
            ProgressLine::Done => msg.push_str(", done"),
        }
        if !options.hide_prefix && requester != rendering.cwd {
            msg = zoom_out(&rendering.cwd, requester, &msg);
        }
        msg
    }
}

impl InstallProgress {
    /// One line on how packages reach `node_modules`, on a first install.
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
        self.context_rendered = true;
        let method = match method {
            PackageImportMethod::Copy => "copied",
            PackageImportMethod::Clone => "cloned",
            PackageImportMethod::Hardlink => "hard linked",
        };
        let virtual_store = normalize(&relative(cwd, &ctx.virtual_store_dir));
        let msg = format!(
            "Packages are {method} from the store at {} into {virtual_store}",
            ctx.store_dir,
        );
        let mut slot = std::mem::take(&mut self.context_slot);
        frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.context_slot = slot;
    }
}
