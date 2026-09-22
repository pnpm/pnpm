use super::{
    BlockSlot,
    COLOR_WHEEL,
    LifecycleEntry,
    LifecycleMessage,
    LifecycleState,
    LifecycleStdio,
    RenderingContext,
    ReporterState,
    contains_path,
    cut_line,
    format_prefix,
    format_prefix_no_trim,
    highlight_last_folder,
    lifecycle_ids,
    pretty_ms,
    visible_width,
};
use std::fmt::Write as _;

impl ReporterState {
    // --- lifecycle --------------------------------------------------------

    pub(super) fn on_lifecycle(&mut self, message: &LifecycleMessage) {
        if (self.options.append_only || self.options.lifecycle.stream_output)
            && !self.options.lifecycle.hide_output
        {
            let Some(msg) = self.streamed_lifecycle_block(message) else { return };
            let mut slot = BlockSlot::default();
            self.display.frame.emit(&mut slot, msg, false);
            return;
        }
        let (stage, dep_path, wd) = lifecycle_ids(message);
        let key = format!("{stage}:{dep_path}");
        let collapsed = contains_path(wd, "/node_modules/") || contains_path(wd, "tmp/_tmp_");
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
        let msg = if self.scripts.entries[&key].collapsed {
            self.render_collapsed(&key, message, dep_path, wd)
        } else {
            self.render_script(&key, message)
        };
        if exit {
            self.scripts.entries.remove(&key);
        }
        let mut slot = self.scripts.slots.remove(&key).unwrap_or_default();
        self.display.frame.emit(&mut slot, msg, false);
        self.scripts.slots.insert(key, slot);
    }

    pub(super) fn update_lifecycle_cache(&mut self, key: &str, message: &LifecycleMessage) {
        match message {
            LifecycleMessage::Script { stage, wd, script, .. } => {
                let line = self.rendering.script_line(stage, wd, script);
                self.scripts.entries.get_mut(key).unwrap().script = line;
            }
            LifecycleMessage::Exit { exit_code, wd, .. } => {
                let status = self.exit_status(key, *exit_code, wd);
                self.scripts.entries.get_mut(key).unwrap().status = status;
            }
            LifecycleMessage::Stdio { line, stdio, .. } => {
                let formatted = self.rendering.format_indented_output(line, *stdio);
                self.scripts.entries
                    .get_mut(key)
                    .unwrap()
                    .output
                    .push(formatted);
            }
        }
    }

    pub(super) fn exit_status(&self, key: &str, exit_code: i32, wd: &str) -> String {
        let time = self.scripts.entries
            .get(key)
            .and_then(|e| e.start)
            .map(|start| pretty_ms(start.elapsed().as_millis()))
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
        let entry = &self.scripts.entries[key];
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
        if self.scripts.entries[key].label.is_none() {
            let mut label = highlight_last_folder(
                &format_prefix_no_trim(&self.rendering.cwd, wd),
                &self.rendering.colors,
            );
            let stage = lifecycle_ids(message).0;
            if contains_path(wd, "tmp/_tmp_") {
                let _ = write!(label, " [{dep_path}]");
            }
            let _ = write!(label, ": Running {stage} script");
            self.scripts.entries.get_mut(key).unwrap().label = Some(label);
        }
        let label = self.scripts.entries[key].label.clone().unwrap();
        let LifecycleMessage::Exit { exit_code, optional, .. } = message else {
            self.update_lifecycle_cache(key, message);
            return format!("{label}...");
        };
        let time = self.scripts.entries
            .get(key)
            .and_then(|e| e.start)
            .map(|start| pretty_ms(start.elapsed().as_millis()))
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
        if !self.options.lifecycle.aggregate_output {
            return Some(self.stream_lifecycle(message));
        }
        let (stage, dep_path, _) = lifecycle_ids(message);
        let key = format!("{stage}:{dep_path}");
        // Format on flush rather than on arrival so the prefix color
        // wheel advances in the order the blocks are printed.
        if !matches!(message, LifecycleMessage::Exit { .. }) {
            self.scripts.buffers
                .entry(key)
                .or_default()
                .push(message.clone());
            return None;
        }
        let mut lines = Vec::new();
        for buffered in self.scripts.buffers.remove(&key).unwrap_or_default() {
            lines.push(self.stream_lifecycle(&buffered));
        }
        lines.push(self.stream_lifecycle(message));
        Some(lines.join("\n"))
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
                let line = match stdio {
                    LifecycleStdio::Stderr => self.rendering.colors.grey(line),
                    LifecycleStdio::Stdout => line.clone(),
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
        let cut = cut_line(line, self.width as isize - 2);
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
