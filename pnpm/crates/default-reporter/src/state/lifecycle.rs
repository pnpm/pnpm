use super::{
    BlockPlacement,
    BlockSlot,
    COLOR_WHEEL,
    LifecycleEntry,
    LifecycleMessage,
    LifecycleState,
    LifecycleStdio,
    RenderingContext,
    ReporterState,
    checks::{
        RunningScript,
        ScriptKind,
    },
    contains_path,
    cut_line,
    format_prefix,
    format_prefix_no_trim,
    highlight_last_folder,
    lifecycle_ids,
    pretty_ms,
    printable_script_line,
    visible_width,
};
use std::fmt::Write as _;

impl ReporterState {
    // --- lifecycle --------------------------------------------------------

    pub(super) fn on_lifecycle(&mut self, message: &LifecycleMessage) {
        self.render_approvals_before_failure(message);
        if self.options.lifecycle.summarize_scripts {
            self.record_script_event(message);
        }
        let quiet = self.options.max_log_level < crate::MaxLogLevel::Info;
        if quiet
            || ((self.options.append_only || self.options.lifecycle.stream_output)
                && !self.options.lifecycle.hide_output)
        {
            let Some(msg) = self.streamed_lifecycle_block(message) else { return };
            let mut slot = BlockSlot::default();
            self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
            return;
        }
        self.render_in_place(message);
    }

    /// Renders a script as a block that later events of the same script
    /// redraw.
    fn render_in_place(&mut self, message: &LifecycleMessage) {
        let (stage, dep_path, wd) = lifecycle_ids(message);
        let key = format!("{stage}:{dep_path}");
        let collapsed = is_dependency_dir(wd);
        let running = self.rendering.format_indented_status(
            &self.rendering.colors.magenta_bright("Running..."),
        );
        let now = std::time::Instant::now();
        self.scripts.entries
            .entry(key.clone())
            .or_insert_with(|| LifecycleEntry {
                collapsed,
                label: None,
                output: Vec::new(),
                script: String::new(),
                status: running,
                start: Some(now),
            });
        let exit = matches!(message, LifecycleMessage::Exit { .. });
        if self.options.lifecycle.summarize_scripts {
            self.collect_summarized(&key, message);
            return;
        }
        let msg = if self.scripts.entries[&key].collapsed {
            self.render_collapsed(&key, message, dep_path, wd)
        } else {
            self.render_script(&key, message)
        };
        if exit {
            self.scripts.entries.remove(&key);
        }
        let mut slot = self.scripts.slots.remove(&key).unwrap_or_default();
        self.display.frame.emit(&mut slot, msg, BlockPlacement::Scrolling);
        self.scripts.slots.insert(key, slot);
    }

    /// Collects a summarized script's output without drawing it, since the
    /// live checks footer shows it running. Only a failure that fails the
    /// install prints, with all of its output.
    fn collect_summarized(&mut self, key: &str, message: &LifecycleMessage) {
        let LifecycleMessage::Exit { exit_code, optional, .. } = message else {
            self.update_lifecycle_cache(key, message);
            return;
        };
        if *exit_code != 0 && !*optional {
            let msg = self.render_script(key, message);
            self.display.frame.emit(&mut BlockSlot::default(), msg, BlockPlacement::Scrolling);
        }
        self.scripts.entries.remove(key);
    }

    fn render_approvals_before_failure(&mut self, message: &LifecycleMessage) {
        if self.options.append_only
            && matches!(message, LifecycleMessage::Exit { exit_code, .. } if *exit_code != 0)
        {
            self.render_pending_approvals();
        }
    }

    pub(super) fn update_lifecycle_cache(&mut self, key: &str, message: &LifecycleMessage) {
        match message {
            LifecycleMessage::Script { stage, wd, script, .. } => {
                let line = self.rendering.script_line(stage, wd, script);
                if let Some(entry) = self.scripts.entries.get_mut(key) {
                    entry.script = line;
                }
            }
            LifecycleMessage::Exit { exit_code, wd, .. } => {
                let status = self.exit_status(key, *exit_code, wd);
                if let Some(entry) = self.scripts.entries.get_mut(key) {
                    entry.status = status;
                }
            }
            LifecycleMessage::Stdio { line, stdio, .. } => {
                let formatted = self.rendering.format_indented_output(line, *stdio);
                if let Some(entry) = self.scripts.entries.get_mut(key) {
                    entry.output.push(formatted);
                }
            }
        }
    }

    pub(super) fn exit_status(&self, key: &str, exit_code: i32, wd: &str) -> String {
        let time = self.scripts.entries
            .get(key)
            .and_then(|e| e.start)
            .map(|start| start.elapsed())
            .map(|elapsed| elapsed.as_millis())
            .map(pretty_ms)
            .unwrap_or_default();
        if exit_code == 0 {
            self.rendering.format_indented_status(&self.rendering.colors.magenta_bright(&format!(
                "Done in {time}",
            )))
        } else {
            self.rendering.format_indented_status(&self.rendering.colors.red(&format!(
                "Failed in {time} at {wd}",
            )))
        }
    }

    pub(super) fn render_script(&mut self, key: &str, message: &LifecycleMessage) -> String {
        self.update_lifecycle_cache(key, message);
        let Some(entry) = self.scripts.entries.get(key) else {
            return String::new();
        };
        let exit_nonzero =
            matches!(message, LifecycleMessage::Exit { exit_code, .. } if *exit_code != 0);
        let mut lines = vec![entry.script.clone()];
        if !exit_nonzero && entry.output.len() > 10 {
            lines.push(format!("[{} lines collapsed]", entry.output.len() - 10));
            lines.extend(entry.output[entry.output.len() - 10..].iter().cloned());
        } else {
            lines.extend(entry.output.iter().cloned());
        }
        lines.push(entry.status.clone());
        lines.join("\n")
    }

    pub(super) fn render_collapsed(
        &mut self,
        key: &str,
        message: &LifecycleMessage,
        dep_path: &str,
        wd: &str,
    ) -> String {
        if let Some(entry) = self.scripts.entries.get(key)
            && entry.label.is_none()
        {
            let mut label = highlight_last_folder(
                &format_prefix_no_trim(&self.rendering.cwd, wd),
                &self.rendering.colors,
            );
            let stage = lifecycle_ids(message).0;
            if contains_path(wd, "tmp/_tmp_") {
                let _ = write!(label, " [{dep_path}]");
            }
            let _ = write!(label, ": Running {stage} script");
            if let Some(entry) = self.scripts.entries.get_mut(key) {
                entry.label = Some(label);
            }
        }
        let label = self.scripts.entries
            .get(key)
            .and_then(|entry| entry.label.clone())
            .unwrap_or_default();
        let LifecycleMessage::Exit { exit_code, optional, .. } = message else {
            self.update_lifecycle_cache(key, message);
            return format!("{label}...");
        };
        let time = self.scripts.entries
            .get(key)
            .and_then(|e| e.start)
            .map(|start| start.elapsed())
            .map(|elapsed| elapsed.as_millis())
            .map(pretty_ms)
            .unwrap_or_default();
        if *exit_code == 0 {
            return format!("{label}, done in {time}");
        }
        if *optional {
            return format!("{label}, failed in {time} (skipped as optional)");
        }
        format!("{label}, failed in {time}\n{}", self.render_script(key, message))
    }

    /// The streamed rendering of one lifecycle event, or `None` when
    /// [`LifecycleOptions::aggregate_output`](crate::state::LifecycleOptions::aggregate_output) is withholding it until the
    /// script exits. The whole run is then returned as one block, so a
    /// concurrent sibling's lines cannot interleave with it.
    pub(super) fn streamed_lifecycle_block(
        &mut self,
        message: &LifecycleMessage,
    ) -> Option<String> {
        let aggregate_output = self.options.lifecycle.aggregate_output
            || self.options.max_log_level < crate::MaxLogLevel::Info;
        if !aggregate_output {
            if self.options.lifecycle.summarize_scripts {
                return self.stream_summarized(message);
            }
            return Some(self.stream_lifecycle(message));
        }
        let (stage, dep_path, wd) = lifecycle_ids(message);
        // `pnpm -r exec` reports a project's manifest name as its
        // `dep_path`, so same-named projects differ only by `wd`.
        let key = format!("{stage}\0{dep_path}\0{wd}");
        // Format on flush rather than on arrival so the prefix color
        // wheel advances in the order the blocks are printed.
        if !matches!(message, LifecycleMessage::Exit { .. }) {
            self.scripts.buffers
                .entry(key)
                .or_default()
                .push(message.clone());
            return None;
        }
        self.finish_streamed_lifecycle(&key, message)
    }

    fn finish_streamed_lifecycle(
        &mut self,
        key: &str,
        message: &LifecycleMessage,
    ) -> Option<String> {
        let buffered = self.scripts.buffers.remove(key).unwrap_or_default();
        let LifecycleMessage::Exit { exit_code, optional, .. } = message else { unreachable!() };
        if self.options.lifecycle.summarize_scripts && *exit_code == 0 {
            return self.summarized_success_block(&buffered);
        }
        if self.withholds_aggregated_exit(*exit_code, *optional) {
            return None;
        }
        let mut lines: Vec<String> = buffered
            .iter()
            .map(|buffered| self.stream_lifecycle(buffered))
            .collect();
        let exit_line = self.stream_lifecycle(message);
        if *optional && self.options.max_log_level < crate::MaxLogLevel::Info {
            lines.push(format!("{exit_line} (skipped as optional)"));
        } else {
            lines.push(exit_line);
        }
        Some(lines.join("\n"))
    }

    /// Whether an aggregated script prints nothing at the configured log
    /// level: a success below `info`, or an optional failure at `error`.
    fn withholds_aggregated_exit(&self, exit_code: i32, optional: bool) -> bool {
        let level = self.options.max_log_level;
        (exit_code == 0 && level < crate::MaxLogLevel::Info)
            || (optional && level == crate::MaxLogLevel::Error)
    }

    /// The aggregated block of a summarized script that succeeded: its
    /// output without the exit line, or nothing when it printed none.
    fn summarized_success_block(&mut self, buffered: &[LifecycleMessage]) -> Option<String> {
        let printed = buffered
            .iter()
            .any(|event| matches!(event, LifecycleMessage::Stdio { .. }));
        if !printed || self.options.max_log_level < crate::MaxLogLevel::Info {
            return None;
        }
        let lines: Vec<String> = buffered
            .iter()
            .map(|buffered| self.stream_lifecycle(buffered))
            .collect();
        Some(lines.join("\n"))
    }

    /// The streamed rendering of a summarized script: its output as it
    /// arrives, with the `$ <script>` line held back until the first output
    /// line or a failure. A success prints no exit line, since the checks
    /// footer lists it.
    fn stream_summarized(&mut self, message: &LifecycleMessage) -> Option<String> {
        let (stage, dep_path, wd) = lifecycle_ids(message);
        let key = format!("{stage}\0{dep_path}\0{wd}");
        if matches!(message, LifecycleMessage::Script { .. }) {
            self.scripts.headers.insert(key, message.clone());
            return None;
        }
        let header = self.scripts.headers.remove(&key);
        if matches!(message, LifecycleMessage::Exit { exit_code: 0, .. }) {
            return None;
        }
        let mut lines: Vec<String> = header
            .iter()
            .map(|header| self.stream_lifecycle(header))
            .collect();
        lines.push(self.stream_lifecycle(message));
        Some(lines.join("\n"))
    }

    /// Records a script for the checks footer: running from its start, and
    /// once it succeeds, as a project script or a dependency build. A
    /// failed optional build is recorded from its
    /// `pnpm:skipped-optional-dependency` event.
    fn record_script_event(&mut self, message: &LifecycleMessage) {
        let (stage, dep_path, wd) = lifecycle_ids(message);
        let (kind, label) = if is_dependency_dir(wd) {
            (ScriptKind::Dependency, dependency_label(dep_path).to_string())
        } else {
            (
                ScriptKind::Project,
                format!("{} {stage}", format_prefix_no_trim(&self.rendering.cwd, wd)),
            )
        };
        let script = RunningScript { kind, id: dep_path, dir: wd, label: &label };
        match message {
            LifecycleMessage::Script { .. } => self.record_script_started(&script),
            LifecycleMessage::Exit { exit_code, .. } => {
                self.record_script_finished(&script);
                if *exit_code != 0 {
                    return;
                }
                match kind {
                    ScriptKind::Dependency => self.record_dependency_built(&script),
                    ScriptKind::Project => self.record_project_script(&label),
                }
            }
            LifecycleMessage::Stdio { .. } => {}
        }
    }

    pub(super) fn stream_lifecycle(&mut self, message: &LifecycleMessage) -> String {
        let (stage, _dep_path, wd) = lifecycle_ids(message);
        let prefix = self.scripts.lifecycle_prefix(&self.rendering, wd, stage);
        match message {
            LifecycleMessage::Exit { exit_code, .. } => {
                if *exit_code == 0 {
                    format!("{prefix}: Done")
                } else {
                    format!("{prefix}: Failed")
                }
            }
            LifecycleMessage::Script { script, .. } => format!("{prefix}$ {script}"),
            LifecycleMessage::Stdio { line, stdio, .. } => {
                let line = printable_script_line(line, self.rendering.colors.enabled);
                let line = match stdio {
                    LifecycleStdio::Stderr => self.rendering.colors.grey(&line),
                    LifecycleStdio::Stdout => line.into_owned(),
                };
                if self.options.lifecycle.hide_prefix { line } else { format!("{prefix}: {line}") }
            }
        }
    }
}

impl RenderingContext {
    pub(super) fn script_line(&self, stage: &str, wd: &str, script: &str) -> String {
        let prefix = format!("{} {}", format_prefix(&self.cwd, wd), self.colors.cyan_bright(stage));
        let max = self.width as isize - visible_width(&prefix) as isize - 2;
        format!("{prefix}$ {}", cut_line(script, max))
    }
}

impl RenderingContext {
    pub(super) fn format_indented_status(&self, status: &str) -> String {
        format!("{} {status}", self.colors.magenta_bright("└─"))
    }
}

impl RenderingContext {
    pub(super) fn format_indented_output(&self, line: &str, stdio: LifecycleStdio) -> String {
        let line = printable_script_line(line, self.colors.enabled);
        let cut = cut_line(&line, self.width as isize - 2);
        let line = match stdio {
            LifecycleStdio::Stderr => self.colors.grey(&cut),
            LifecycleStdio::Stdout => cut,
        };
        format!("{} {line}", self.colors.magenta_bright("│"))
    }
}

impl LifecycleState {
    pub(super) fn lifecycle_prefix(
        &mut self,
        rendering: &RenderingContext,
        wd: &str,
        stage: &str,
    ) -> String {
        let idx = if let Some(idx) = self.colors.get(wd) {
            *idx
        } else {
            let idx = self.color_wheel % COLOR_WHEEL.len();
            self.colors.insert(wd.to_string(), idx);
            self.color_wheel += 1;
            idx
        };
        let painted = COLOR_WHEEL[idx](&rendering.colors, &format_prefix(&rendering.cwd, wd));
        format!("{painted} {}", rendering.colors.cyan_bright(stage))
    }
}

fn is_dependency_dir(wd: &str) -> bool {
    contains_path(wd, "/node_modules/") || contains_path(wd, "tmp/_tmp_")
}

/// `name@version` of a dependency's lockfile key, without its peer and
/// patch suffixes.
fn dependency_label(dep_path: &str) -> &str {
    dep_path
        .split_once('(')
        .map(|(label, _)| label)
        .unwrap_or(dep_path)
}
