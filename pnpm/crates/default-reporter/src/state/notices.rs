use super::{
    BTreeMap,
    BTreeSet,
    BlockPlacement,
    Colors,
    CommandOutcome,
    DedupeCheckLog,
    ExecutionTimeLog,
    Frame,
    HookLog,
    IgnoredScriptsLog,
    InstallingConfigDepsLog,
    InstallingConfigDepsStatus,
    LockfileVerificationMessage,
    LogLevel,
    MAX_SHOWN_WARNINGS,
    MaxLogLevel,
    NoticeState,
    PendingSkillsLog,
    RESOLUTION_SKIPPED,
    ReporterState,
    RequestRetryLog,
    SkippedOptionalDependencyLog,
    SkippedOptionalPackage,
    SkippedOptionalReason,
    UnusedOverrideLog,
    UpdateCheckLog,
    cached_verdict,
    detect_install_source,
    is_strictly_newer,
    normalize,
    pending_approval_packages,
    pretty_ms,
    progress_label,
    relative,
    update_command,
    zoom_out,
};
use jiff::Timestamp;

fn format_pending_approval_packages(packages: &BTreeMap<String, BTreeSet<&str>>) -> String {
    packages
        .iter()
        .map(|(name, capabilities)| {
            format!(
                "  {name}  {}",
                capabilities
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl ReporterState {
    // --- misc one-liners --------------------------------------------------

    pub(super) fn on_ignored_scripts(&mut self, log: &IgnoredScriptsLog) {
        // Suppress the warning box under `strictDepBuilds`, the install
        // fails with `ERR_PNPM_IGNORED_BUILDS` instead, so the box would
        // only duplicate the error. The structured event still carries the
        // names for NDJSON consumers.
        if !log.strict_dep_builds {
            self.notices.approvals.add_builds(&log.package_names);
        }
        if !self.options.append_only {
            self.render_pending_approvals();
        }
    }

    pub(super) fn on_pending_skills(&mut self, log: &PendingSkillsLog) {
        self.notices.approvals.add_skills(&log.package_names);
        if !self.options.append_only {
            self.render_pending_approvals();
        }
    }

    pub(super) fn render_pending_approvals(&mut self) {
        if self.options.append_only && self.notices.approvals.emitted {
            return;
        }
        let packages = pending_approval_packages(
            &self.notices.approvals.builds,
            &self.notices.approvals.skills,
        );
        if packages.is_empty() {
            return;
        }
        let list = format_pending_approval_packages(&packages);
        let approve = r#"Run "pnpm approve" to review the packages awaiting approval."#;
        let build_instruction = self.options.ignored_builds_instruction_text
            .as_deref()
            .filter(|_| !self.notices.approvals.builds.is_empty());
        let instruction = match build_instruction {
            Some(text) if self.notices.approvals.skills.is_empty() => text.to_string(),
            Some(text) => format!("{text}\n{approve}"),
            None => approve.to_string(),
        };
        self.display.frame.emit(
            &mut self.notices.approvals.slot,
            format!("Packages awaiting approval:\n{list}\n\n{instruction}"),
            BlockPlacement::Scrolling,
        );
        if self.options.append_only {
            self.notices.approvals.emitted = true;
        }
    }

    /// pnpm's `reportUpdateCheck`: tell the user a newer pnpm exists and
    /// how to get it. Silent unless the resolved `latest` really is newer
    /// than the running version.
    pub(super) fn on_update_check(&mut self, log: &UpdateCheckLog) {
        if !is_strictly_newer(&log.latest_version, &log.current_version) {
            return;
        }
        self.display.frame.push_block(format!(
            "Update available! {current} → {latest}.\n{changelog} https://pnpm.io/v/{version}\nTo update, run: {command}",
            current = self.rendering.colors.red(&log.current_version),
            latest = self.rendering.colors.green(&log.latest_version),
            changelog = self.rendering.colors.magenta("Changelog:"),
            version = log.latest_version,
            command = self.rendering.colors.magenta(&update_command(detect_install_source())),
        ));
    }

    pub(super) fn on_config_deps(&mut self, log: &InstallingConfigDepsLog) {
        let msg = match log.status {
            InstallingConfigDepsStatus::Started => "Installing config dependencies...".to_string(),
            InstallingConfigDepsStatus::Done => {
                let list = log.deps
                    .iter()
                    .map(|dep| format!("{}@{}", dep.name, dep.version))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Installed config dependencies: {list}")
            }
        };
        let mut slot = std::mem::take(&mut self.display.config_deps_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.display.config_deps_slot = slot;
    }

    /// A passing verdict goes to the checks footer, and the in-place
    /// progress block is cleared.
    pub(super) fn on_lockfile_verification(&mut self, message: &LockfileVerificationMessage) {
        if matches!(
            message,
            LockfileVerificationMessage::Cached { .. } | LockfileVerificationMessage::Done { .. },
        ) {
            self.record_supply_chain_passed();
            self.display.frame.clear(&mut self.display.lockfile_verification_slot);
            return;
        }
        // Append-only output cannot erase a line, so it prints only a
        // failure: progress would flood CI logs, and a passing verdict goes
        // to the checks footer. In-place mode re-renders the block instead.
        if self.options.append_only
            && !matches!(message, LockfileVerificationMessage::Failed { .. })
        {
            return;
        }
        let msg = self.lockfile_verification_line(message);
        let mut slot = std::mem::take(&mut self.display.lockfile_verification_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.display.lockfile_verification_slot = slot;
    }

    fn lockfile_verification_line(&self, message: &LockfileVerificationMessage) -> String {
        let lockfile_path = match message {
            LockfileVerificationMessage::Cached { lockfile_path, .. }
            | LockfileVerificationMessage::Started { lockfile_path, .. }
            | LockfileVerificationMessage::Progress { lockfile_path, .. }
            | LockfileVerificationMessage::Done { lockfile_path, .. }
            | LockfileVerificationMessage::Failed { lockfile_path, .. } => lockfile_path,
        };
        let path = self.lockfile_path_suffix(lockfile_path.as_deref());
        match message {
            LockfileVerificationMessage::Cached { verified_at, .. } => format!(
                "{} Lockfile{path} passes supply-chain policies ({})",
                self.rendering.colors.green("✓"),
                cached_verdict(verified_at.as_deref(), Timestamp::now()),
            ),
            LockfileVerificationMessage::Started { entries, .. } => format!(
                "{} Verifying lockfile{path} against supply-chain policies ({})...",
                self.rendering.colors.cyan("?"),
                progress_label(0, *entries),
            ),
            LockfileVerificationMessage::Progress { entries, checked, .. } => format!(
                "{} Verifying lockfile{path} against supply-chain policies ({})...",
                self.rendering.colors.cyan("?"),
                progress_label(*checked, *entries),
            ),
            LockfileVerificationMessage::Done { entries, checked, elapsed_ms, .. } => format!(
                "{} Lockfile{path} passes supply-chain policies ({} in {})",
                self.rendering.colors.green("✓"),
                progress_label(*checked, *entries),
                pretty_ms(u128::from(*elapsed_ms)),
            ),
            LockfileVerificationMessage::Failed { entries, checked, elapsed_ms, .. } => format!(
                "{} Lockfile{path} failed supply-chain policy check ({} in {})",
                self.rendering.colors.red("✗"),
                progress_label(*checked, *entries),
                pretty_ms(u128::from(*elapsed_ms)),
            ),
        }
    }

    pub(super) fn lockfile_path_suffix(&self, lockfile_path: Option<&str>) -> String {
        let Some(path) = lockfile_path else { return String::new() };
        let from_expected = relative(&self.rendering.cwd, path);
        let is_direct_child = !from_expected.contains('/') && !from_expected.starts_with("..");
        if is_direct_child {
            return String::new();
        }
        format!(" at {}", normalize(&relative(&self.rendering.cwd, path)))
    }

    pub(super) fn on_request_retry(&mut self, log: &RequestRetryLog) {
        let left = log.max_retries.saturating_sub(log.attempt);
        #[cfg_attr(
            dylint_lib = "perfectionist",
            expect(
                perfectionist::em_dash_prose,
                reason = "the retry notice keeps its printed separator"
            )
        )]
        let msg = format!(
            "{} {} error ({}) {} {}\nWill retry in {}. {left} retries left.",
            log.method,
            log.url,
            log.error.message,
            "—",
            log.attempt,
            pretty_ms(u128::from(log.timeout)),
        );
        self.notices.push_warning(
            self.rendering.colors,
            &mut self.display.frame,
            self.options.append_only,
            &msg,
        );
    }

    pub(super) fn on_pnpm(&mut self, level: LogLevel, message: &str, prefix: &str) {
        if level == LogLevel::Error && self.options.append_only {
            self.render_pending_approvals();
        }
        match level {
            LogLevel::Debug if self.options.max_log_level >= MaxLogLevel::Debug => {
                self.display.frame.push_block(message.to_string());
            }
            LogLevel::Warn if self.options.max_log_level >= MaxLogLevel::Warn => {
                self.notices.push_warning(
                    self.rendering.colors,
                    &mut self.display.frame,
                    self.options.append_only,
                    message,
                );
            }
            LogLevel::Error => self.display.frame.push_block(message.to_string()),
            LogLevel::Info if self.options.max_log_level >= MaxLogLevel::Info => {
                self.on_info(message, prefix);
            }
            LogLevel::Debug | LogLevel::Warn | LogLevel::Info => {}
        }
    }

    /// A prefixed info line belongs to another project's reporter, so only
    /// the current project's own lines render.
    pub(super) fn on_info(&mut self, message: &str, prefix: &str) {
        if !prefix.is_empty() && prefix != self.rendering.cwd {
            return;
        }
        if message == RESOLUTION_SKIPPED {
            self.record_resolution_skipped();
        } else {
            self.display.frame.push_block(message.to_string());
        }
    }

    pub(super) fn on_dedupe_check(&mut self, log: &DedupeCheckLog) {
        self.display.frame.push_block(format!("\n{}", log.rendered));
    }

    pub(super) fn on_execution_time(&mut self, log: &ExecutionTimeLog) {
        self.render_up_to_date();
        self.render_checks(CommandOutcome::Succeeded);
        if self.options.append_only {
            self.render_pending_approvals();
        }
        if self.options.max_log_level < MaxLogLevel::Info {
            return;
        }
        let elapsed = log.ended_at.saturating_sub(log.started_at);
        let msg =
            format!("Done in {} using pnpm v{}", pretty_ms(elapsed), crate::package_version());
        let mut slot = std::mem::take(&mut self.display.exec_slot);
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Pinned);
        self.display.exec_slot = slot;
    }

    /// Mirrors pnpm's `reportSkippedOptionalDependencies`: a package that
    /// could not be fetched renders as a warning wherever it sits in the
    /// graph and whichever project the install runs from, since it is
    /// missing for the whole install. Any other skip renders only when it
    /// belongs to the current project and its `parents` chain is present
    /// and empty (a direct optional dependency); transitive and
    /// parent-less skips stay debug-only.
    pub(super) fn on_skipped_optional(&mut self, log: &SkippedOptionalDependencyLog) {
        if log.reason == SkippedOptionalReason::FetchFailure {
            self.on_skipped_optional_fetch_failure(log);
            return;
        }
        if log.prefix != self.rendering.cwd || !log.parents.as_ref().is_some_and(Vec::is_empty) {
            return;
        }
        let message = match &log.package {
            SkippedOptionalPackage::Installed { id, .. } => format!(
                "info: {id} is an optional dependency and failed compatibility check. Excluding it from installation.",
            ),
            SkippedOptionalPackage::ResolutionFailure { name, bare_specifier, .. } => {
                let pkg = match name {
                    Some(name) => format!("{name}@{bare_specifier}"),
                    None => bare_specifier.clone(),
                };
                format!(
                    "info: {pkg} is an optional dependency that could not be resolved. Excluding it from installation.",
                )
            }
        };
        self.display.frame.push_block(message);
    }

    fn on_skipped_optional_fetch_failure(&mut self, log: &SkippedOptionalDependencyLog) {
        if self.options.max_log_level < MaxLogLevel::Warn {
            return;
        }
        let pkg = match &log.package {
            SkippedOptionalPackage::Installed { name, version, .. } => format!("{name}@{version}"),
            SkippedOptionalPackage::ResolutionFailure { bare_specifier, .. } => {
                bare_specifier.clone()
            }
        };
        let mut message = format!(
            "{pkg} is an optional dependency that could not be fetched. Excluding it from installation.",
        );
        if let Some(details) = &log.details {
            message.push('\n');
            message.push_str(details);
        }
        self.notices.push_warning(
            self.rendering.colors,
            &mut self.display.frame,
            self.options.append_only,
            &message,
        );
    }

    pub(super) fn on_unused_override(&mut self, log: &UnusedOverrideLog) {
        self.notices.unused_overrides.push(log.selector.clone());
    }

    /// One grouped, sorted warning for every selector buffered since the
    /// last `resolution_done`. Selectors come from `pnpm-workspace.yaml`, which
    /// a contributor can edit, so they are sanitized before rendering.
    pub(super) fn flush_unused_overrides(&mut self) {
        if self.notices.unused_overrides.is_empty() {
            return;
        }
        let mut selectors: Vec<String> = std::mem::take(&mut self.notices.unused_overrides)
            .iter()
            .map(String::as_str)
            .map(pnpm_text_sanitize::sanitize_inline)
            .map(std::borrow::Cow::into_owned)
            .collect();
        selectors.sort();
        let head = if selectors.len() == 1 {
            "1 override matched no dependency".to_string()
        } else {
            format!("{} overrides matched no dependency", selectors.len())
        };
        self.notices.push_warning(
            self.rendering.colors,
            &mut self.display.frame,
            self.options.append_only,
            &format!("{head}: {}", selectors.join(", ")),
        );
    }

    /// Renders a `pnpm:hook` event as `hook: message`, matching pnpm's
    /// `reportHooks.ts` format. When the hook's `prefix` differs from
    /// `self.rendering.cwd` the message is zoomed out with the prefix.
    pub(super) fn on_hook(&mut self, log: &HookLog) {
        let msg = format!("{}: {}", self.rendering.colors.magenta_bright(&log.hook), log.message);
        if log.prefix.is_empty() || log.prefix == self.rendering.cwd {
            self.display.frame.push_block(msg);
        } else {
            let zoomed = zoom_out(&self.rendering.cwd, &log.prefix, &msg);
            self.display.frame.push_block(zoomed);
        }
    }
}

impl NoticeState {
    /// A warning, honoring pnpm's "only show the first
    /// [`MAX_SHOWN_WARNINGS`], then collapse the rest into a count" rule.
    pub(super) fn push_warning(
        &mut self,
        colors: Colors,
        frame: &mut Frame,
        append_only: bool,
        message: &str,
    ) {
        self.warnings += 1;
        if append_only || self.warnings <= MAX_SHOWN_WARNINGS {
            frame.push_block(format!("{} {message}", colors.warn_label()));
            return;
        }
        let extra = self.warnings - MAX_SHOWN_WARNINGS;
        let msg = format!("{} {extra} other warnings", colors.warn_label());
        let mut slot = std::mem::take(&mut self.collapsed_slot);
        frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.collapsed_slot = slot;
    }
}
