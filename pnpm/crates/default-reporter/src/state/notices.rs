use super::{
    Colors, DedupeCheckLog, DeprecationLog, ExecutionTimeLog, Frame, HookLog, IgnoredScriptsLog,
    InstallingConfigDepsLog, InstallingConfigDepsStatus, LockfileVerificationMessage, LogLevel,
    MAX_SHOWN_WARNINGS, MaxLogLevel, NoticeState, ReporterState, RequestRetryLog,
    SkippedOptionalDependencyLog, SkippedOptionalPackage, UpdateCheckLog, Utc, cached_verdict,
    detect_install_source, is_strictly_newer, normalize, pretty_ms, progress_label, relative,
    update_command, zoom_out,
};

impl ReporterState {
    // --- misc one-liners --------------------------------------------------

    pub(super) fn on_ignored_scripts(&mut self, log: &IgnoredScriptsLog) {
        if log.package_names.is_empty() {
            return;
        }
        // Suppress the warning box under `strictDepBuilds` — the install
        // fails with `ERR_PNPM_IGNORED_BUILDS` instead, so the box would
        // only duplicate the error. The box is gated on
        // `!strictDepBuilds`; the structured event still carries the
        // names for NDJSON consumers.
        if log.strict_dep_builds {
            return;
        }
        let list = log.package_names.join(", ");
        let instruction = self.options.ignored_builds_instruction_text.as_deref().unwrap_or(
            r#"Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts."#,
        );
        self.display.frame.push_block(format!("Ignored build scripts: {list}.\n{instruction}"));
    }

    /// pnpm's `reportUpdateCheck`: tell the user a newer pnpm exists and
    /// how to get it. Silent unless the resolved `latest` really is newer
    /// than the running version.
    pub(super) fn on_update_check(&mut self, log: &UpdateCheckLog) {
        if !is_strictly_newer(&log.latest_version, &log.current_version) {
            return;
        }
        self.display.frame.push_block(format!(
            "Update available! {current} \u{2192} {latest}.\n{changelog} https://pnpm.io/v/{version}\nTo update, run: {command}",
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
        self.display.frame.emit(&mut slot, msg, false);
        self.display.config_deps_slot = slot;
    }

    pub(super) fn on_lockfile_verification(&mut self, message: &LockfileVerificationMessage) {
        // Append-only output prints one line per event, so the throttled
        // progress stream would flood CI logs; the terminal verdict
        // carries the final count. In-place mode re-renders the
        // verification block instead.
        if self.options.append_only
            && matches!(message, LockfileVerificationMessage::Progress { .. })
        {
            return;
        }
        let msg = self.lockfile_verification_line(message);
        let mut slot = std::mem::take(&mut self.display.lockfile_verification_slot);
        self.display.frame.emit(&mut slot, msg, false);
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
                cached_verdict(verified_at.as_deref(), Utc::now()),
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
        if message == "Lockfile is up to date, resolution step is skipped" {
            self.display.pending_lockfile_message = Some(message.to_string());
        } else {
            self.display.frame.push_block(message.to_string());
        }
    }

    pub(super) fn flush_pending_lockfile_message(&mut self) {
        if let Some(message) = self.display.pending_lockfile_message.take() {
            self.display.frame.push_block(message);
        }
    }

    pub(super) fn on_dedupe_check(&mut self, log: &DedupeCheckLog) {
        self.display.frame.push_block(format!("\n{}", log.rendered));
    }

    pub(super) fn on_execution_time(&mut self, log: &ExecutionTimeLog) {
        let elapsed = log.ended_at.saturating_sub(log.started_at);
        let msg =
            format!("Done in {} using pnpm v{}", pretty_ms(elapsed), crate::package_version());
        let mut slot = std::mem::take(&mut self.display.exec_slot);
        self.display.frame.emit(&mut slot, msg, true);
        self.display.exec_slot = slot;
    }

    /// Mirrors pnpm's `reportSkippedOptionalDependencies`: only a skip
    /// whose `parents` chain is present and empty (a direct optional
    /// dependency of the current project) renders; transitive and
    /// parent-less skips stay debug-only.
    pub(super) fn on_skipped_optional(&mut self, log: &SkippedOptionalDependencyLog) {
        if log.prefix != self.rendering.cwd || !log.parents.as_ref().is_some_and(Vec::is_empty) {
            return;
        }
        let pkg = match &log.package {
            SkippedOptionalPackage::Installed { id, .. } => id.clone(),
            SkippedOptionalPackage::ResolutionFailure {
                name: Some(name),
                version: Some(version),
                ..
            } => format!("{name}@{version}"),
            SkippedOptionalPackage::ResolutionFailure { bare_specifier, .. } => {
                bare_specifier.clone()
            }
        };
        self.display.frame.push_block(format!(
            "info: {pkg} is an optional dependency and failed compatibility check. Excluding it from installation.",
        ));
    }

    /// Matches pnpm's `reportDeprecations.ts`: only direct-dependency
    /// deprecations render immediately; transitive ones wait for the
    /// `resolution_done` summary.
    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        if log.depth == 0 {
            if !self.options.scope.recursive && log.prefix == self.rendering.cwd {
                self.display.frame.push_block(format!(
                    "{} {} {}@{}: {}",
                    self.rendering.colors.warn_label(),
                    self.rendering.colors.red("deprecated"),
                    log.pkg_name,
                    log.pkg_version,
                    log.deprecated,
                ));
            } else {
                // The zoomed line drops the deprecation text, as
                // `reportDeprecations.ts` does.
                let msg = format!(
                    "{} {} {}@{}",
                    self.rendering.colors.warn_label(),
                    self.rendering.colors.red("deprecated"),
                    log.pkg_name,
                    log.pkg_version,
                );
                self.display.frame.push_block(zoom_out(&self.rendering.cwd, &log.prefix, &msg));
            }
        } else {
            self.notices.deprecated_subdeps.push(log.clone());
        }
    }

    pub(super) fn flush_deprecated_subdeps(&mut self) {
        if self.notices.deprecated_subdeps.is_empty() {
            return;
        }
        let mut names: Vec<String> = self.notices.deprecated_subdeps
            .iter()
            .map(|log| format!("{}@{}", log.pkg_name, log.pkg_version))
            .collect();
        names.sort();
        let count = names.len();
        let msg = format!(
            "{} {} {}",
            self.rendering.colors.warn_label(),
            self.rendering.colors.red(&format!("{count} deprecated subdependencies found:")),
            names.join(", "),
        );
        self.display.frame.emit(&mut self.notices.deprecated_slot, msg, false);
        self.notices.deprecated_subdeps.clear();
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

    /// Mirrors pnpm's `reportPeerDependencyIssues`: the detail lives in
    /// the event payload, and the terminal gets one line naming the
    /// command that prints it. Upstream `take(1)`s the stream, so a
    /// recursive run that installs several projects still warns once.
    pub(super) fn on_peer_dependency_issues(&mut self) {
        if std::mem::replace(&mut self.notices.reported_peer_issues, true) {
            return;
        }
        self.notices.push_warning(
            self.rendering.colors,
            &mut self.display.frame,
            self.options.append_only,
            r#"Issues with peer dependencies found. Run "pnpm peers check" to list them."#,
        );
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
        frame.emit(&mut slot, msg, false);
        self.collapsed_slot = slot;
    }
}
