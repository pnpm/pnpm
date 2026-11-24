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
    ProgressStats,
    ReporterState,
    ScopeLog,
    Stage,
    StatsMessage,
    normalize,
    pretty_bytes,
    relative,
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

    /// Progress is one live `Packages:` line in the place the final stats
    /// take: the projects that report progress are summed into it.
    pub(super) fn on_progress(&mut self, message: &ProgressMessage) {
        let requester = match message {
            ProgressMessage::Resolved { requester, .. }
            | ProgressMessage::Fetched { requester, .. }
            | ProgressMessage::FoundInStore { requester, .. }
            | ProgressMessage::Imported { requester, .. } => requester.clone(),
        };
        // Its packages are not in the stats, and it never finishes
        // importing, so it would hold the stats back and leave signs that
        // never turn green.
        if requester == pnpm_reporter::PYTHON_ENVIRONMENT_REQUESTER {
            return;
        }
        let entry = self.downloads.progress.entry(requester).or_default();
        if entry.done {
            return;
        }
        match message {
            ProgressMessage::Resolved { .. } => entry.stats.resolved += 1,
            ProgressMessage::Fetched { .. } => entry.stats.fetched += 1,
            ProgressMessage::FoundInStore { .. } => entry.stats.reused += 1,
            ProgressMessage::Imported { .. } => entry.stats.imported += 1,
        }
        self.render_live_packages();
    }

    /// Draws the live line, until the stats replace it.
    fn render_live_packages(&mut self) {
        if self.install.stats_shown.is_some() {
            return;
        }
        let Some(msg) = self.live_packages_message() else { return };
        let mut slot = std::mem::take(&mut self.install.stats_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Pinned);
        self.install.stats_slot = slot;
    }

    /// The live `Packages:` line, with a progress bar below it in a
    /// terminal: see [`Self::live_bar`]. The line follows the bar:
    /// `Packages: 58 resolved` before any package arrives, `Packages: 90
    /// downloaded, 5 reused` while they arrive, and `Packages: +12 of 95`
    /// once packages are added. `None` while there is nothing to count.
    fn live_packages_message(&self) -> Option<String> {
        let totals = self.downloads.totals();
        let line = self.live_packages_line(totals)?;
        // Append-only output prints each update as a new line, so a bar
        // would repeat on every one.
        if self.options.append_only || totals.fetched + totals.reused == 0 {
            return Some(line);
        }
        Some(format!("{line}\n{}", self.live_bar(self.rendering.width)))
    }

    fn live_packages_line(&self, totals: ProgressStats) -> Option<String> {
        let colors = &self.rendering.colors;
        let arrived = totals.fetched + totals.reused;
        if totals.imported > 0 && !self.options.progress.hide_added_pkgs {
            let added = colors.green(&format!("+{}", totals.imported));
            // A package added without a download or reuse event still
            // counts, so the total never trails the added count.
            let total = arrived.max(totals.imported);
            return Some(format!(
                "Packages: {added} of {}",
                colors.cyan_bright(&total.to_string()),
            ));
        }
        let counts = [(totals.fetched, "downloaded"), (totals.reused, "reused")];
        let arriving: Vec<String> = counts
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{} {label}", colors.cyan_bright(&count.to_string())))
            .collect();
        if !arriving.is_empty() {
            return Some(format!("Packages: {}", arriving.join(", ")));
        }
        (totals.resolved > 0).then(|| {
            format!("Packages: {} resolved", colors.cyan_bright(&totals.resolved.to_string()))
        })
    }

    /// A sign for each package that arrived: a grey `·` until the package
    /// is added, then a green `↓` for downloaded or `+` for reused from the
    /// store. Packages are added in parallel, so the green signs scatter
    /// across the bar in a fixed shuffle instead of filling it from the
    /// left. The bar ends as the green bar of the stats. Until then at
    /// least one sign stays grey, so it never looks finished early.
    fn live_bar(&self, max_width: usize) -> String {
        let totals = self.downloads.totals();
        let arrived = totals.fetched + totals.reused;
        if max_width == 0 || arrived == 0 {
            return String::new();
        }
        let chars = usize::try_from(arrived).map_or(max_width, |arrived| arrived.min(max_width));
        let added_chars = usize::try_from(totals.imported * chars as u64 / arrived)
            .map_or(chars, |added| added.min(chars - 1));
        let mut added = vec![false; chars];
        for position in shuffled_positions(chars).into_iter().take(added_chars) {
            added[position] = true;
        }
        let downloaded_chars = self.downloads.downloaded_part(chars);
        let colors = &self.rendering.colors;
        (0..chars)
            .map(|position| {
                let sign = if position < downloaded_chars { "↓" } else { "+" };
                if added[position] { colors.green(sign) } else { colors.grey("·") }
            })
            .collect()
    }

    pub(super) fn on_stage(&mut self, prefix: &str, stage: Stage) {
        match stage {
            Stage::ResolutionDone => self.flush_unused_overrides(),
            Stage::ImportingDone => {
                let Some(entry) = self.downloads.progress.get_mut(prefix) else { return };
                entry.done = true;
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

    /// The stats an install's progress ends with: a line such as
    /// `Packages: +5 -2 (60% downloaded)`, and a bar below it with a sign
    /// per package: `↓` for downloaded, `+` for reused from the store, and
    /// `-` for removed. The signs tell the sources apart without color. The line gives the larger share of
    /// downloaded and reused packages, not their counts: resolution also
    /// fetches versions it then drops, so a count would not match the
    /// packages added.
    /// Waits for both halves of every project's stats, and shows each
    /// total once.
    pub(super) fn render_stats(&mut self) {
        let Some((added, removed)) = self.install.stats_totals() else { return };
        if self.install.stats_shown.replace((added, removed)) == Some((added, removed)) {
            return;
        }
        if added == 0 && removed == 0 {
            self.display.frame.clear(&mut self.install.stats_slot);
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
        msg.push('\n');
        msg.push_str(&self.pluses_and_minuses(self.rendering.width, added, removed));
        let mut slot = std::mem::take(&mut self.install.stats_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.install.stats_slot = slot;
    }

    pub(super) fn pluses_and_minuses(&self, max_width: usize, added: u64, removed: u64) -> String {
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
        let downloaded_chars = self.downloads.downloaded_part(added_chars);
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

/// The positions `0..len` in a fixed shuffled order: a position's place
/// depends only on the position, so the order is the same on every redraw.
fn shuffled_positions(len: usize) -> Vec<usize> {
    let mut positions: Vec<usize> = (0..len).collect();
    positions.sort_by_key(|position| scramble(*position as u64));
    positions
}

/// splitmix64's finalizer: spreads consecutive inputs across the range.
fn scramble(input: u64) -> u64 {
    let mut value = input.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
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
        let (downloaded, reused) = self.package_sources();
        let total = downloaded + reused;
        if total == 0 {
            return None;
        }
        let (share, source) =
            if downloaded >= reused { (downloaded, "downloaded") } else { (reused, "reused") };
        Some(format!("{}% {source}", share * 100 / total))
    }

    /// How many packages were downloaded and how many reused from the
    /// store.
    fn package_sources(&self) -> (u64, u64) {
        self.progress
            .values()
            .fold((0, 0), |(downloaded, reused), entry| {
                (downloaded + entry.stats.fetched, reused + entry.stats.reused)
            })
    }

    /// How many of the bar's `chars` added signs are downloaded `↓`
    /// packages, in the share the line reports. A download keeps at
    /// least one sign of its own. Zero when the sources are unknown.
    fn downloaded_part(&self, chars: usize) -> usize {
        let (downloaded, reused) = self.package_sources();
        let total = downloaded + reused;
        if total == 0 {
            return 0;
        }
        let part = usize::try_from(downloaded * chars as u64 / total).unwrap_or(chars);
        if downloaded > 0 { part.max(1).min(chars) } else { part }
    }

    /// The progress of every project, summed.
    fn totals(&self) -> ProgressStats {
        self.progress
            .values()
            .fold(ProgressStats::default(), |sum, entry| ProgressStats {
                resolved: sum.resolved + entry.stats.resolved,
                reused: sum.reused + entry.stats.reused,
                fetched: sum.fetched + entry.stats.fetched,
                imported: sum.imported + entry.stats.imported,
            })
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
