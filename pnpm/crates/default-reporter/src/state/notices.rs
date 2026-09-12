use super::{
    DedupeCheckLog, DeprecationLog, ExecutionTimeLog, HookLog, IgnoredScriptsLog,
    InstallingConfigDepsLog, InstallingConfigDepsStatus, LockfileVerificationMessage, LogLevel,
    MAX_SHOWN_WARNINGS, MaxLogLevel, ReporterState, RequestRetryLog, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, UpdateCheckLog, Utc, cached_verdict, detect_install_source,
    is_strictly_newer, normalize, pretty_ms, progress_label, relative, update_command, zoom_out,
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
        self.push_block(format!("Ignored build scripts: {list}.\n{instruction}"));
    }

    /// pnpm's `reportUpdateCheck`: tell the user a newer pnpm exists and
    /// how to get it. Silent unless the resolved `latest` really is newer
    /// than the running version.
    pub(super) fn on_update_check(&mut self, log: &UpdateCheckLog) {
        if !is_strictly_newer(&log.latest_version, &log.current_version) {
            return;
        }
        self.push_block(format!(
            "Update available! {current} \u{2192} {latest}.\n{changelog} https://pnpm.io/v/{version}\nTo update, run: {command}",
            current = self.colors.red(&log.current_version),
            latest = self.colors.green(&log.latest_version),
            changelog = self.colors.magenta("Changelog:"),
            version = log.latest_version,
            command = self.colors.magenta(&update_command(detect_install_source())),
        ));
    }

    pub(super) fn on_config_deps(&mut self, log: &InstallingConfigDepsLog) {
        let msg = match log.status {
            InstallingConfigDepsStatus::Started => "Installing config dependencies...".to_string(),
            InstallingConfigDepsStatus::Done => {
                let list = log
                    .deps
                    .iter()
                    .map(|dep| format!("{}@{}", dep.name, dep.version))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Installed config dependencies: {list}")
            }
        };
        let mut slot = std::mem::take(&mut self.config_deps_slot);
        self.frame.emit(&mut slot, msg, false);
        self.config_deps_slot = slot;
    }

    pub(super) fn on_lockfile_verification(&mut self, message: &LockfileVerificationMessage) {
        let msg = match message {
            LockfileVerificationMessage::Cached { verified_at, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} passes supply-chain policies ({})",
                    self.colors.green("✓"),
                    cached_verdict(verified_at.as_deref(), Utc::now()),
                )
            }
            LockfileVerificationMessage::Started { entries, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Verifying lockfile{path} against supply-chain policies ({})...",
                    self.colors.cyan("?"),
                    progress_label(0, *entries),
                )
            }
            LockfileVerificationMessage::Done { entries, checked, elapsed_ms, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} passes supply-chain policies ({} in {})",
                    self.colors.green("✓"),
                    progress_label(*checked, *entries),
                    pretty_ms(u128::from(*elapsed_ms)),
                )
            }
            LockfileVerificationMessage::Failed { entries, checked, elapsed_ms, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} failed supply-chain policy check ({} in {})",
                    self.colors.red("✗"),
                    progress_label(*checked, *entries),
                    pretty_ms(u128::from(*elapsed_ms)),
                )
            }
        };
        let mut slot = std::mem::take(&mut self.lockfile_verification_slot);
        self.frame.emit(&mut slot, msg, false);
        self.lockfile_verification_slot = slot;
    }

    pub(super) fn lockfile_path_suffix(&self, lockfile_path: Option<&str>) -> String {
        let Some(path) = lockfile_path else { return String::new() };
        let from_expected = relative(&self.cwd, path);
        let is_direct_child = !from_expected.contains('/') && !from_expected.starts_with("..");
        if is_direct_child {
            return String::new();
        }
        format!(" at {}", normalize(&relative(&self.cwd, path)))
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
        self.push_warning(&msg);
    }

    pub(super) fn on_pnpm(&mut self, level: LogLevel, message: &str, prefix: &str) {
        match level {
            LogLevel::Debug if self.options.max_log_level >= MaxLogLevel::Debug => {
                self.push_block(message.to_string());
            }
            LogLevel::Warn if self.options.max_log_level >= MaxLogLevel::Warn => {
                self.push_warning(message);
            }
            LogLevel::Error => self.push_block(message.to_string()),
            LogLevel::Info if self.options.max_log_level >= MaxLogLevel::Info => {
                self.on_info(message, prefix);
            }
            LogLevel::Debug | LogLevel::Warn | LogLevel::Info => {}
        }
    }

    /// A prefixed info line belongs to another project's reporter, so only
    /// the current project's own lines render.
    pub(super) fn on_info(&mut self, message: &str, prefix: &str) {
        if !prefix.is_empty() && prefix != self.cwd {
            return;
        }
        if message == "Lockfile is up to date, resolution step is skipped" {
            self.pending_lockfile_message = Some(message.to_string());
        } else {
            self.push_block(message.to_string());
        }
    }

    pub(super) fn flush_pending_lockfile_message(&mut self) {
        if let Some(message) = self.pending_lockfile_message.take() {
            self.push_block(message);
        }
    }

    pub(super) fn on_dedupe_check(&mut self, log: &DedupeCheckLog) {
        self.push_block(format!("\n{}", log.rendered));
    }

    pub(super) fn on_execution_time(&mut self, log: &ExecutionTimeLog) {
        let elapsed = log.ended_at.saturating_sub(log.started_at);
        let msg =
            format!("Done in {} using pnpm v{}", pretty_ms(elapsed), crate::package_version());
        let mut slot = std::mem::take(&mut self.exec_slot);
        self.frame.emit(&mut slot, msg, true);
        self.exec_slot = slot;
    }

    /// Mirrors pnpm's `reportSkippedOptionalDependencies`: only a skip
    /// whose `parents` chain is present and empty (a direct optional
    /// dependency of the current project) renders; transitive and
    /// parent-less skips stay debug-only.
    pub(super) fn on_skipped_optional(&mut self, log: &SkippedOptionalDependencyLog) {
        if log.prefix != self.cwd || !log.parents.as_ref().is_some_and(Vec::is_empty) {
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
        self.push_block(format!(
            "info: {pkg} is an optional dependency and failed compatibility check. Excluding it from installation.",
        ));
    }

    /// Matches pnpm's `reportDeprecations.ts`: only direct-dependency
    /// deprecations render immediately; transitive ones wait for the
    /// `resolution_done` summary.
    pub(super) fn on_deprecation(&mut self, log: &DeprecationLog) {
        if log.depth == 0 {
            if !self.options.is_recursive && log.prefix == self.cwd {
                self.push_block(format!(
                    "{} {} {}@{}: {}",
                    self.colors.warn_label(),
                    self.colors.red("deprecated"),
                    log.pkg_name,
                    log.pkg_version,
                    log.deprecated,
                ));
            } else {
                // The zoomed line drops the deprecation text, as
                // `reportDeprecations.ts` does.
                let msg = format!(
                    "{} {} {}@{}",
                    self.colors.warn_label(),
                    self.colors.red("deprecated"),
                    log.pkg_name,
                    log.pkg_version,
                );
                self.push_block(zoom_out(&self.cwd, &log.prefix, &msg));
            }
        } else {
            self.deprecated_subdeps.push(log.clone());
        }
    }

    pub(super) fn flush_deprecated_subdeps(&mut self) {
        if self.deprecated_subdeps.is_empty() {
            return;
        }
        let mut names: Vec<String> = self
            .deprecated_subdeps
            .iter()
            .map(|log| format!("{}@{}", log.pkg_name, log.pkg_version))
            .collect();
        names.sort();
        let count = names.len();
        let msg = format!(
            "{} {} {}",
            self.colors.warn_label(),
            self.colors.red(&format!("{count} deprecated subdependencies found:")),
            names.join(", "),
        );
        self.frame.emit(&mut self.deprecated_slot, msg, false);
        self.deprecated_subdeps.clear();
    }

    /// Renders a `pnpm:hook` event as `hook: message`, matching pnpm's
    /// `reportHooks.ts` format. When the hook's `prefix` differs from
    /// `self.cwd` the message is zoomed out with the prefix.
    pub(super) fn on_hook(&mut self, log: &HookLog) {
        let msg = format!("{}: {}", self.colors.magenta_bright(&log.hook), log.message);
        if log.prefix.is_empty() || log.prefix == self.cwd {
            self.push_block(msg);
        } else {
            let zoomed = zoom_out(&self.cwd, &log.prefix, &msg);
            self.push_block(zoomed);
        }
    }

    /// Mirrors pnpm's `reportPeerDependencyIssues`: the detail lives in
    /// the event payload, and the terminal gets one line naming the
    /// command that prints it. Upstream `take(1)`s the stream, so a
    /// recursive run that installs several projects still warns once.
    pub(super) fn on_peer_dependency_issues(&mut self) {
        if std::mem::replace(&mut self.reported_peer_dependency_issues, true) {
            return;
        }
        self.push_warning(
            r#"Issues with peer dependencies found. Run "pnpm peers check" to list them."#,
        );
    }

    /// A warning, honoring pnpm's "only show the first
    /// [`MAX_SHOWN_WARNINGS`], then collapse the rest into a count" rule.
    pub(super) fn push_warning(&mut self, message: &str) {
        self.warnings_counter += 1;
        if self.options.append_only || self.warnings_counter <= MAX_SHOWN_WARNINGS {
            self.push_block(format!("{} {message}", self.colors.warn_label()));
            return;
        }
        let extra = self.warnings_counter - MAX_SHOWN_WARNINGS;
        let msg = format!("{} {extra} other warnings", self.colors.warn_label());
        let mut slot = std::mem::take(&mut self.collapsed_warn_slot);
        self.frame.emit(&mut slot, msg, false);
        self.collapsed_warn_slot = slot;
    }
}
