use super::{
    BigTarball, BlockSlot, ContextLog, FetchingProgressMessage, PackageImportMethod,
    ProgressMessage, ReporterState, ScopeLog, Stage, StatsMessage, normalize, pretty_bytes,
    relative, zoom_out,
};

impl ReporterState {
    // --- scope ------------------------------------------------------------

    /// pnpm's `reportScope`: how many workspace projects the command
    /// selected. Silent for a command that doesn't report scope, and for a
    /// single selected project — where the answer is the directory the
    /// user is already standing in.
    pub(super) fn on_scope(&mut self, log: &ScopeLog) {
        if !self.options.reports_scope || log.selected == 1 {
            return;
        }
        let count = match log.total {
            Some(total) if total == log.selected => format!("all {total}"),
            Some(total) => format!("{} of {total}", log.selected),
            None => log.selected.to_string(),
        };
        let unit = if log.workspace_prefix.is_some() { "workspace projects" } else { "projects" };
        let mut slot = std::mem::take(&mut self.scope_slot);
        self.frame.emit(&mut slot, format!("Scope: {count} {unit}"), false);
        self.scope_slot = slot;
    }

    // --- context ----------------------------------------------------------

    pub(super) fn on_context(&mut self, log: &ContextLog) {
        self.context = Some(log.clone());
        self.maybe_render_context();
    }

    pub(super) fn maybe_render_context(&mut self) {
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
        let virtual_store = normalize(&relative(&self.cwd, &ctx.virtual_store_dir));
        let msg = format!(
            "Packages are {method} from the content-addressable store to the virtual store.\n  \
             Content-addressable store is at: {}\n  Virtual store is at:             {}",
            ctx.store_dir, virtual_store,
        );
        self.context_rendered = true;
        let mut slot = std::mem::take(&mut self.context_slot);
        self.frame.emit(&mut slot, msg, false);
        self.context_slot = slot;
    }

    // --- progress ---------------------------------------------------------

    pub(super) fn on_progress(&mut self, message: &ProgressMessage) {
        let requester = match message {
            ProgressMessage::Resolved { requester, .. }
            | ProgressMessage::Fetched { requester, .. }
            | ProgressMessage::FoundInStore { requester, .. }
            | ProgressMessage::Imported { requester, .. } => requester.clone(),
        };
        let entry = self.progress.entry(requester.clone()).or_default();
        match message {
            ProgressMessage::Resolved { .. } => entry.stats.resolved += 1,
            ProgressMessage::Fetched { .. } => entry.stats.fetched += 1,
            ProgressMessage::FoundInStore { .. } => entry.stats.reused += 1,
            ProgressMessage::Imported { .. } => entry.stats.imported += 1,
        }
        let msg = self.progress_message(&requester, false);
        let mut slot = std::mem::take(&mut self.progress.get_mut(&requester).unwrap().slot);
        self.frame.emit(&mut slot, msg, true);
        self.progress.get_mut(&requester).unwrap().slot = slot;
    }

    pub(super) fn progress_message(&self, requester: &str, done: bool) -> String {
        let stats = self.progress.get(requester).map(|entry| entry.stats).unwrap_or_default();
        let hl = |count: u64| self.colors.cyan_bright(&count.to_string());
        let mut msg = format!(
            "Progress: resolved {}, reused {}, downloaded {}",
            hl(stats.resolved),
            hl(stats.reused),
            hl(stats.fetched),
        );
        if !self.options.hide_added_pkgs_progress {
            msg.push_str(", added ");
            msg.push_str(&hl(stats.imported));
        }
        if done {
            msg.push_str(", done");
        }
        if !self.options.hide_progress_prefix && requester != self.cwd {
            msg = zoom_out(&self.cwd, requester, &msg);
        }
        msg
    }

    pub(super) fn on_stage(&mut self, prefix: &str, stage: Stage) {
        match stage {
            Stage::ResolutionDone => {
                self.flush_deprecated_subdeps();
            }
            Stage::ImportingDone => {
                if !self.progress.contains_key(prefix) {
                    return;
                }
                let msg = self.progress_message(prefix, true);
                let mut slot = std::mem::take(&mut self.progress.get_mut(prefix).unwrap().slot);
                self.frame.emit(&mut slot, msg, false);
                self.progress.get_mut(prefix).unwrap().slot = slot;
            }
            _ => {}
        }
    }

    // --- big tarballs -----------------------------------------------------

    pub(super) fn on_fetching(&mut self, message: &FetchingProgressMessage) {
        const BIG_TARBALL_SIZE: u64 = 1024 * 1024 * 5;
        match message {
            FetchingProgressMessage::Started { attempt, package_id, size } => {
                let Some(size) = size else { return };
                if *size < BIG_TARBALL_SIZE || *attempt != 1 {
                    return;
                }
                let mut entry = BigTarball { size: *size, slot: BlockSlot::default() };
                let msg = self.downloading_message(package_id, 0, *size);
                self.frame.emit(&mut entry.slot, msg, true);
                self.big.insert(package_id.clone(), entry);
            }
            FetchingProgressMessage::InProgress { downloaded, package_id } => {
                let Some(entry) = self.big.get(package_id) else { return };
                let size = entry.size;
                let done = *downloaded == size;
                let msg = self.downloading_message(package_id, *downloaded, size);
                let mut slot = std::mem::take(&mut self.big.get_mut(package_id).unwrap().slot);
                self.frame.emit(&mut slot, msg, !done);
                self.big.get_mut(package_id).unwrap().slot = slot;
            }
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
            self.colors.cyan_bright(&pretty_bytes(downloaded)),
            self.colors.cyan_bright(&pretty_bytes(size)),
        )
    }

    // --- stats ------------------------------------------------------------

    pub(super) fn on_stats(&mut self, message: &StatsMessage) {
        let prefix = match message {
            StatsMessage::Added { prefix, .. } | StatsMessage::Removed { prefix, .. } => prefix,
        };
        if prefix != &self.cwd {
            return;
        }
        match message {
            StatsMessage::Added { added, .. } => {
                self.stats_added = Some(*added);
            }
            StatsMessage::Removed { removed, .. } => {
                self.stats_removed = Some(*removed);
            }
        }
        if self.stats_added.is_some() && self.stats_removed.is_some() {
            self.render_stats();
        }
    }

    pub(super) fn render_stats(&mut self) {
        let added = self.stats_added.take().unwrap_or(0);
        let removed = self.stats_removed.take().unwrap_or(0);
        if added == 0 && removed == 0 {
            let mut slot = std::mem::take(&mut self.stats_slot);
            self.frame.emit(&mut slot, "Already up to date".to_string(), false);
            self.stats_slot = slot;
            return;
        }
        let mut msg = String::from("Packages:");
        if added > 0 {
            msg.push(' ');
            msg.push_str(&self.colors.green(&format!("+{added}")));
        }
        if removed > 0 {
            msg.push(' ');
            msg.push_str(&self.colors.red(&format!("-{removed}")));
        }
        msg.push('\n');
        msg.push_str(&self.pluses_and_minuses(self.width, added, removed));
        let mut slot = std::mem::take(&mut self.stats_slot);
        self.frame.emit(&mut slot, msg, false);
        self.stats_slot = slot;
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
        let mut out = String::new();
        for _ in 0..added_chars {
            out.push_str(&self.colors.green("+"));
        }
        for _ in 0..removed_chars {
            out.push_str(&self.colors.red("-"));
        }
        out
    }
}
