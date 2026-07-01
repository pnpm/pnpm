//! Event-folding renderer: the in-process equivalent of
//! `@pnpm/cli.default-reporter`'s `RxJS` graph. Each [`LogEvent`] is folded into
//! [`ReporterState`], which recomputes the terminal frame. The frame model
//! pins fixed blocks below scrolling non-fixed blocks, with one rendering
//! path per log channel.

use std::{collections::HashMap, fmt::Write as _};

use pacquet_reporter::{
    AddedRoot, ContextLog, DependencyType, ExecutionTimeLog, FetchingProgressMessage,
    IgnoredScriptsLog, InstallingConfigDepsLog, InstallingConfigDepsStatus, LifecycleMessage,
    LifecycleStdio, LockfileVerificationMessage, LogEvent, LogLevel, PackageImportMethod,
    PackageManifestMessage, ProgressMessage, RemovedRoot, RequestRetryLog, SkippedOptionalPackage,
    Stage, StatsMessage,
};
use serde_json::Value;

use crate::{
    colors::Colors,
    format::{
        contains_path, cut_line, format_prefix, format_prefix_no_trim, highlight_last_folder,
        normalize, pretty_bytes, pretty_ms, relative, visible_width, zoom_out,
    },
};

/// What [`ReporterState::handle`] asks the sink to do after folding one event.
pub enum Output {
    /// Nothing changed; the sink writes nothing.
    None,
    /// The full recomputed frame (in-place mode).
    Frame(String),
    /// Lines to append verbatim (append-only mode).
    Lines(Vec<String>),
}

/// Lazily-assigned block indices for one logical output stream — its
/// non-fixed (`block`) and fixed (`fixed`) slots in the frame — the
/// per-stream `currentBlockNo` / `currentFixedBlockNo` pair.
#[derive(Debug, Default, Clone)]
struct BlockSlot {
    block: Option<usize>,
    fixed: Option<usize>,
}

/// The frame buffer: scrolling `blocks` rendered above pinned `fixed_blocks`,
/// or — in append-only mode — a list of `pending` lines to print as they
/// arrive.
#[derive(Debug)]
struct Frame {
    append_only: bool,
    blocks: Vec<Option<String>>,
    fixed_blocks: Vec<Option<String>>,
    next_block: usize,
    next_fixed: usize,
    pending: Vec<String>,
}

impl Frame {
    fn new(append_only: bool) -> Self {
        Frame {
            append_only,
            blocks: Vec::new(),
            fixed_blocks: Vec::new(),
            next_block: 0,
            next_fixed: 0,
            pending: Vec::new(),
        }
    }

    fn emit(&mut self, slot: &mut BlockSlot, msg: String, fixed: bool) {
        if self.append_only {
            self.pending.push(msg);
            return;
        }
        if fixed {
            let idx = *slot.fixed.get_or_insert_with(|| {
                let assigned = self.next_fixed;
                self.next_fixed += 1;
                assigned
            });
            if self.fixed_blocks.len() <= idx {
                self.fixed_blocks.resize(idx + 1, None);
            }
            self.fixed_blocks[idx] = Some(msg);
        } else {
            if let Some(f) = slot.fixed.take() {
                self.fixed_blocks[f] = None;
            }
            let idx = *slot.block.get_or_insert_with(|| {
                let assigned = self.next_block;
                self.next_block += 1;
                assigned
            });
            if self.blocks.len() <= idx {
                self.blocks.resize(idx + 1, None);
            }
            self.blocks[idx] = Some(msg);
        }
    }

    fn render(&self) -> String {
        let non_fixed: Vec<&str> = self.blocks.iter().filter_map(|b| b.as_deref()).collect();
        let fixed: Vec<&str> = self.fixed_blocks.iter().filter_map(|b| b.as_deref()).collect();
        let non_fixed_part = non_fixed.join("\n");
        if fixed.is_empty() {
            return non_fixed_part;
        }
        let fixed_part = fixed.join("\n");
        if non_fixed_part.is_empty() {
            return fixed_part;
        }
        format!("{non_fixed_part}\n{fixed_part}")
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct ProgressStats {
    resolved: u64,
    reused: u64,
    fetched: u64,
    imported: u64,
}

#[derive(Debug, Default)]
struct ProgressEntry {
    stats: ProgressStats,
    slot: BlockSlot,
}

/// One dependency added or removed, ready to render.
#[derive(Debug, Clone)]
struct PackageDiff {
    added: bool,
    from: Option<String>,
    name: String,
    real_name: Option<String>,
    version: Option<String>,
    latest: Option<String>,
}

/// The five dependency buckets, in summary render order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DepKind {
    Prod,
    Optional,
    Peer,
    Dev,
    NodeModulesOnly,
}

const SUMMARY_ORDER: [DepKind; 5] =
    [DepKind::Prod, DepKind::Optional, DepKind::Peer, DepKind::Dev, DepKind::NodeModulesOnly];

impl DepKind {
    fn header(self) -> &'static str {
        match self {
            DepKind::Prod => "dependencies",
            DepKind::Optional => "optionalDependencies",
            DepKind::Peer => "peerDependencies",
            DepKind::Dev => "devDependencies",
            DepKind::NodeModulesOnly => "node_modules",
        }
    }

    fn from_dependency_type(dt: Option<DependencyType>) -> Self {
        match dt {
            Some(DependencyType::Prod) => DepKind::Prod,
            Some(DependencyType::Dev) => DepKind::Dev,
            Some(DependencyType::Optional) => DepKind::Optional,
            None => DepKind::NodeModulesOnly,
        }
    }
}

#[derive(Debug, Default)]
struct LifecycleEntry {
    collapsed: bool,
    label: Option<String>,
    output: Vec<String>,
    script: String,
    status: String,
    start: Option<std::time::Instant>,
}

#[derive(Debug)]
struct BigTarball {
    size: u64,
    slot: BlockSlot,
}

/// The whole renderer state. One instance lives behind the sink's mutex in
/// production; tests construct it directly.
pub struct ReporterState {
    cwd: String,
    width: usize,
    colors: Colors,
    append_only: bool,
    frame: Frame,
    last_frame: Option<String>,

    progress: HashMap<String, ProgressEntry>,

    context: Option<ContextLog>,
    import_method: Option<PackageImportMethod>,
    context_slot: BlockSlot,
    context_rendered: bool,

    stats_added: Option<u64>,
    stats_removed: Option<u64>,
    stats_slot: BlockSlot,

    diff: HashMap<&'static str, HashMap<String, PackageDiff>>,
    manifest_initial: Option<Value>,
    manifest_updated: Option<Value>,
    summary_slot: BlockSlot,
    summary_rendered: bool,

    lifecycle: HashMap<String, LifecycleEntry>,
    lifecycle_slots: HashMap<String, BlockSlot>,
    lifecycle_colors: HashMap<String, usize>,
    color_wheel: usize,

    big: HashMap<String, BigTarball>,

    config_deps_slot: BlockSlot,
    lockfile_verification_slot: BlockSlot,
    exec_slot: BlockSlot,

    warnings_counter: usize,
    collapsed_warn_slot: BlockSlot,

    /// `pnpm:unusedOverride` selectors buffered until
    /// `pnpm:stage { stage: "resolution_done" }`, mirroring pnpm's
    /// `reportDeprecations.ts` `buffer(resolutionDone$)` shape. When
    /// the stage fires, the buffered selectors are concatenated into
    /// one grouped warning and the buffer is cleared.
    pending_unused_overrides: Vec<String>,
}

const MAX_SHOWN_WARNINGS: usize = 5;

/// Lifecycle-script prefix color wheel.
const COLOR_WHEEL: [fn(&Colors, &str) -> String; 6] = [
    |colors, text| colors.cyan(text),
    |colors, text| colors.magenta_bright(text),
    // chalk's `blue` has no dedicated helper here; bright_cyan is the closest
    // already-mapped tone and keeps the wheel visually distinct.
    |colors, text| colors.cyan_bright(text),
    |colors, text| colors.yellow(text),
    |colors, text| colors.green(text),
    |colors, text| colors.red(text),
];

impl ReporterState {
    #[must_use]
    pub fn new(cwd: String, width: usize, colors: Colors, append_only: bool) -> Self {
        let mut diff = HashMap::new();
        for kind in SUMMARY_ORDER {
            diff.insert(diff_key(kind), HashMap::new());
        }
        ReporterState {
            cwd,
            width,
            colors,
            append_only,
            frame: Frame::new(append_only),
            last_frame: None,
            progress: HashMap::new(),
            context: None,
            import_method: None,
            context_slot: BlockSlot::default(),
            context_rendered: false,
            stats_added: None,
            stats_removed: None,
            stats_slot: BlockSlot::default(),
            diff,
            manifest_initial: None,
            manifest_updated: None,
            summary_slot: BlockSlot::default(),
            summary_rendered: false,
            lifecycle: HashMap::new(),
            lifecycle_slots: HashMap::new(),
            lifecycle_colors: HashMap::new(),
            color_wheel: 0,
            big: HashMap::new(),
            config_deps_slot: BlockSlot::default(),
            lockfile_verification_slot: BlockSlot::default(),
            exec_slot: BlockSlot::default(),
            warnings_counter: 0,
            collapsed_warn_slot: BlockSlot::default(),
            pending_unused_overrides: Vec::new(),
        }
    }

    pub fn handle(&mut self, event: &LogEvent) -> Output {
        match event {
            LogEvent::Context(log) => self.on_context(log),
            LogEvent::PackageImportMethod(log) => {
                self.import_method = Some(log.method);
                self.maybe_render_context();
            }
            LogEvent::Progress(log) => self.on_progress(&log.message),
            LogEvent::Stage(log) => self.on_stage(&log.prefix, log.stage),
            LogEvent::FetchingProgress(log) => self.on_fetching(&log.message),
            LogEvent::Stats(log) => self.on_stats(&log.message),
            LogEvent::Root(log) => self.on_root(&log.message),
            LogEvent::PackageManifest(log) => self.on_manifest(&log.message),
            LogEvent::Summary(_) => self.on_summary(),
            LogEvent::Lifecycle(log) => self.on_lifecycle(&log.message),
            LogEvent::IgnoredScripts(log) => self.on_ignored_scripts(log),
            LogEvent::SkippedOptionalDependency(log) => {
                let pkg = match &log.package {
                    SkippedOptionalPackage::Installed { id, .. } => id.clone(),
                    SkippedOptionalPackage::ResolutionFailure { bare_specifier, .. } => {
                        bare_specifier.clone()
                    }
                };
                self.push_warning(&format!("Skipping optional dependency {pkg}"));
            }
            LogEvent::InstallingConfigDeps(log) => self.on_config_deps(log),
            LogEvent::LockfileVerification(log) => self.on_lockfile_verification(&log.message),
            LogEvent::RequestRetry(log) => self.on_request_retry(log),
            LogEvent::Pnpm(log) => self.on_pnpm(log.level, &log.message, &log.prefix),
            // `pnpm:global` shares the "other" log stream with the `pnpm`
            // channel but carries no prefix, so it always renders (the
            // empty-prefix path in `on_pnpm`).
            LogEvent::Global(log) => self.on_pnpm(log.level, &log.message, ""),
            LogEvent::ExecutionTime(log) => self.on_execution_time(log),
            LogEvent::UnusedOverride(log) => {
                self.pending_unused_overrides.push(log.selector.clone());
            }
            // Debug-only / non-rendered channels in pnpm's default reporter.
            LogEvent::Hook(_) | LogEvent::BrokenModules(_) => {}
        }
        self.finish()
    }

    fn finish(&mut self) -> Output {
        if self.append_only {
            let lines = std::mem::take(&mut self.frame.pending);
            if lines.is_empty() { Output::None } else { Output::Lines(lines) }
        } else {
            let frame = self.frame.render();
            if self.last_frame.as_deref() == Some(frame.as_str()) {
                Output::None
            } else {
                self.last_frame = Some(frame.clone());
                Output::Frame(frame)
            }
        }
    }

    // --- context ----------------------------------------------------------

    fn on_context(&mut self, log: &ContextLog) {
        self.context = Some(log.clone());
        self.maybe_render_context();
    }

    fn maybe_render_context(&mut self) {
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

    fn on_progress(&mut self, message: &ProgressMessage) {
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

    fn progress_message(&self, requester: &str, done: bool) -> String {
        let stats = self.progress.get(requester).map(|entry| entry.stats).unwrap_or_default();
        let hl = |count: u64| self.colors.cyan_bright(&count.to_string());
        let mut msg = format!(
            "Progress: resolved {}, reused {}, downloaded {}, added {}",
            hl(stats.resolved),
            hl(stats.reused),
            hl(stats.fetched),
            hl(stats.imported),
        );
        if done {
            msg.push_str(", done");
        }
        if requester != self.cwd {
            msg = zoom_out(&self.cwd, requester, &msg);
        }
        msg
    }

    fn on_stage(&mut self, prefix: &str, stage: Stage) {
        if stage == Stage::ResolutionDone {
            self.flush_pending_unused_overrides();
            return;
        }
        if stage != Stage::ImportingDone {
            return;
        }
        if !self.progress.contains_key(prefix) {
            return;
        }
        let msg = self.progress_message(prefix, true);
        let mut slot = std::mem::take(&mut self.progress.get_mut(prefix).unwrap().slot);
        self.frame.emit(&mut slot, msg, false);
        self.progress.get_mut(prefix).unwrap().slot = slot;
    }

    /// Emit a single grouped warning for any override selectors that
    /// matched no resolved dependency. Mirrors pnpm's
    /// `reportUnusedOverrides.ts`: the count word is singular only
    /// when exactly one was collected, and the whole batch is emitted
    /// at `resolution_done` rather than streamed per event. The buffer
    /// is cleared after the flush so subsequent installs in the same
    /// reporter process start fresh.
    ///
    /// Selectors are sanitized (control characters stripped) before
    /// rendering so a crafted override key containing `\n`, `\r`, or
    /// ESC cannot inject/spoof terminal output. The raw selector stays
    /// intact in the structured `LogEvent::UnusedOverride` payload.
    /// Selectors arrive pre-sorted from the emission site
    /// (`install_with_fresh_lockfile`), so no re-sort is needed here.
    fn flush_pending_unused_overrides(&mut self) {
        if self.pending_unused_overrides.is_empty() {
            return;
        }
        let selectors = std::mem::take(&mut self.pending_unused_overrides);
        let sanitized: Vec<String> =
            selectors.iter().map(|s| sanitize_override_selector(s)).collect();
        let head = if sanitized.len() == 1 {
            "1 override matched no dependency".to_string()
        } else {
            format!("{} overrides matched no dependency", sanitized.len())
        };
        self.push_warning(&format!("{}: {}", head, sanitized.join(", ")));
    }

    // --- big tarballs -----------------------------------------------------

    fn on_fetching(&mut self, message: &FetchingProgressMessage) {
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

    fn downloading_message(&self, package_id: &str, downloaded: u64, size: u64) -> String {
        let done = downloaded == size;
        let suffix = if done { ", done" } else { "" };
        format!(
            "Downloading {package_id}: {}/{}{suffix}",
            self.colors.cyan_bright(&pretty_bytes(downloaded)),
            self.colors.cyan_bright(&pretty_bytes(size)),
        )
    }

    // --- stats ------------------------------------------------------------

    fn on_stats(&mut self, message: &StatsMessage) {
        let prefix = match message {
            StatsMessage::Added { prefix, added } => {
                self.stats_added = Some(*added);
                prefix.clone()
            }
            StatsMessage::Removed { prefix, removed } => {
                self.stats_removed = Some(*removed);
                prefix.clone()
            }
        };
        if prefix != self.cwd {
            return;
        }
        let added = self.stats_added.unwrap_or(0);
        let removed = self.stats_removed.unwrap_or(0);
        if added == 0 && removed == 0 {
            // The "Already up to date" line is emitted by pacquet as a
            // `pnpm` log; rendering it here too would duplicate it.
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

    fn pluses_and_minuses(&self, max_width: usize, added: u64, removed: u64) -> String {
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

    // --- summary ----------------------------------------------------------

    fn on_root(&mut self, message: &pacquet_reporter::RootMessage) {
        use pacquet_reporter::RootMessage;
        let (added, kind, name, version, real_name, from, latest) = match message {
            RootMessage::Added { added, .. } => added_fields(added),
            RootMessage::Removed { removed, .. } => removed_fields(removed),
        };
        let key = diff_key(kind);
        let opposite_key = format!("{}{}", if added { '-' } else { '+' }, name);
        if let Some(prev) = self.diff.get(key).and_then(|b| b.get(&opposite_key))
            && prev.version == version
        {
            self.diff.get_mut(key).unwrap().remove(&opposite_key);
            return;
        }
        let entry = PackageDiff { added, from, name: name.clone(), real_name, version, latest };
        self.diff
            .get_mut(key)
            .unwrap()
            .insert(format!("{}{name}", if added { '+' } else { '-' }), entry);
    }

    fn on_manifest(&mut self, message: &PackageManifestMessage) {
        match message {
            PackageManifestMessage::Initial { initial, .. } => {
                self.manifest_initial = Some(initial.clone());
            }
            PackageManifestMessage::Updated { updated, .. } => {
                self.manifest_updated = Some(updated.clone());
            }
        }
    }

    fn on_summary(&mut self) {
        if self.summary_rendered {
            return;
        }
        self.summary_rendered = true;
        self.apply_manifest_diff();
        let msg = self.render_summary();
        let mut slot = std::mem::take(&mut self.summary_slot);
        self.frame.emit(&mut slot, msg, false);
        self.summary_slot = slot;
    }

    fn apply_manifest_diff(&mut self) {
        let (Some(initial), Some(updated)) =
            (self.manifest_initial.as_ref(), self.manifest_updated.as_ref())
        else {
            return;
        };
        let initial = remove_optional_from_prod(initial);
        let updated = remove_optional_from_prod(updated);
        for kind in [DepKind::Peer, DepKind::Prod, DepKind::Optional, DepKind::Dev] {
            let prop = kind.header();
            let initial_deps = manifest_dep_versions(&initial, prop);
            let updated_deps = manifest_dep_versions(&updated, prop);
            let bucket = self.diff.get_mut(diff_key(kind)).unwrap();
            for (name, version) in &initial_deps {
                if !updated_deps.contains_key(name) {
                    bucket.entry(format!("-{name}")).or_insert_with(|| PackageDiff {
                        added: false,
                        from: None,
                        name: name.clone(),
                        real_name: None,
                        version: Some(version.clone()),
                        latest: None,
                    });
                }
            }
            for (name, version) in &updated_deps {
                if !initial_deps.contains_key(name) {
                    bucket.entry(format!("+{name}")).or_insert_with(|| PackageDiff {
                        added: true,
                        from: None,
                        name: name.clone(),
                        real_name: None,
                        version: Some(version.clone()),
                        latest: None,
                    });
                }
            }
        }
    }

    fn render_summary(&self) -> String {
        let mut msg = String::new();
        for kind in SUMMARY_ORDER {
            let bucket = &self.diff[diff_key(kind)];
            if bucket.is_empty() {
                continue;
            }
            let mut diffs: Vec<&PackageDiff> = bucket.values().collect();
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

    fn diff_line(&self, pkg: &PackageDiff) -> String {
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
            if let Some(latest) = &pkg.latest
                && latest != version
            {
                result.push(' ');
                result.push_str(&self.colors.grey(&format!("({latest} is available)")));
            }
        }
        if let Some(from) = &pkg.from {
            let rel = relative(&self.cwd, from);
            let shown = if rel.is_empty() { from.clone() } else { rel };
            result.push(' ');
            result.push_str(&self.colors.grey(&format!("<- {shown}")));
        }
        result
    }

    // --- lifecycle --------------------------------------------------------

    fn on_lifecycle(&mut self, message: &LifecycleMessage) {
        if self.append_only {
            let msg = self.stream_lifecycle(message);
            let mut slot = BlockSlot::default();
            self.frame.emit(&mut slot, msg, false);
            return;
        }
        let (stage, dep_path, wd) = lifecycle_ids(message);
        let key = format!("{stage}:{dep_path}");
        let collapsed = contains_path(wd, "/node_modules/") || contains_path(wd, "tmp/_tmp_");
        let running = self.format_indented_status(&self.colors.magenta_bright("Running..."));
        let now = std::time::Instant::now();
        self.lifecycle.entry(key.clone()).or_insert_with(|| LifecycleEntry {
            collapsed,
            label: None,
            output: Vec::new(),
            script: String::new(),
            status: running,
            start: Some(now),
        });
        let exit = matches!(message, LifecycleMessage::Exit { .. });
        let msg = if self.lifecycle[&key].collapsed {
            self.render_collapsed(&key, message, dep_path, wd)
        } else {
            self.render_script(&key, message)
        };
        if exit {
            self.lifecycle.remove(&key);
        }
        let mut slot = self.lifecycle_slots.remove(&key).unwrap_or_default();
        self.frame.emit(&mut slot, msg, false);
        self.lifecycle_slots.insert(key, slot);
    }

    fn update_lifecycle_cache(&mut self, key: &str, message: &LifecycleMessage) {
        match message {
            LifecycleMessage::Script { stage, wd, script, .. } => {
                let prefix =
                    format!("{} {}", format_prefix(&self.cwd, wd), self.colors.cyan_bright(stage));
                let max = self.width as isize - visible_width(&prefix) as isize - 2;
                let line = format!("{prefix}$ {}", cut_line(script, max));
                self.lifecycle.get_mut(key).unwrap().script = line;
            }
            LifecycleMessage::Exit { exit_code, wd, .. } => {
                let time = self
                    .lifecycle
                    .get(key)
                    .and_then(|e| e.start)
                    .map(|start| pretty_ms(start.elapsed().as_millis()))
                    .unwrap_or_default();
                let status = if *exit_code == 0 {
                    self.format_indented_status(
                        &self.colors.magenta_bright(&format!("Done in {time}")),
                    )
                } else {
                    self.format_indented_status(
                        &self.colors.red(&format!("Failed in {time} at {wd}")),
                    )
                };
                self.lifecycle.get_mut(key).unwrap().status = status;
            }
            LifecycleMessage::Stdio { line, stdio, .. } => {
                let formatted = self.format_indented_output(line, *stdio);
                self.lifecycle.get_mut(key).unwrap().output.push(formatted);
            }
        }
    }

    fn render_script(&mut self, key: &str, message: &LifecycleMessage) -> String {
        self.update_lifecycle_cache(key, message);
        let entry = &self.lifecycle[key];
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

    fn render_collapsed(
        &mut self,
        key: &str,
        message: &LifecycleMessage,
        dep_path: &str,
        wd: &str,
    ) -> String {
        if self.lifecycle[key].label.is_none() {
            let mut label =
                highlight_last_folder(&format_prefix_no_trim(&self.cwd, wd), &self.colors);
            let stage = lifecycle_ids(message).0;
            if contains_path(wd, "tmp/_tmp_") {
                let _ = write!(label, " [{dep_path}]");
            }
            let _ = write!(label, ": Running {stage} script");
            self.lifecycle.get_mut(key).unwrap().label = Some(label);
        }
        let label = self.lifecycle[key].label.clone().unwrap();
        let LifecycleMessage::Exit { exit_code, optional, .. } = message else {
            self.update_lifecycle_cache(key, message);
            return format!("{label}...");
        };
        let time = self
            .lifecycle
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

    fn stream_lifecycle(&mut self, message: &LifecycleMessage) -> String {
        let (stage, _dep_path, wd) = lifecycle_ids(message);
        let prefix = self.lifecycle_prefix(wd, stage);
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
                    LifecycleStdio::Stderr => self.colors.grey(line),
                    LifecycleStdio::Stdout => line.clone(),
                };
                format!("{prefix}: {line}")
            }
        }
    }

    fn lifecycle_prefix(&mut self, wd: &str, stage: &str) -> String {
        let idx = if let Some(idx) = self.lifecycle_colors.get(wd) {
            *idx
        } else {
            let idx = self.color_wheel % COLOR_WHEEL.len();
            self.lifecycle_colors.insert(wd.to_string(), idx);
            self.color_wheel += 1;
            idx
        };
        let painted = COLOR_WHEEL[idx](&self.colors, &format_prefix(&self.cwd, wd));
        format!("{painted} {}", self.colors.cyan_bright(stage))
    }

    fn format_indented_status(&self, status: &str) -> String {
        format!("{} {status}", self.colors.magenta_bright("└─"))
    }

    fn format_indented_output(&self, line: &str, stdio: LifecycleStdio) -> String {
        let cut = cut_line(line, self.width as isize - 2);
        let line = match stdio {
            LifecycleStdio::Stderr => self.colors.grey(&cut),
            LifecycleStdio::Stdout => cut,
        };
        format!("{} {line}", self.colors.magenta_bright("│"))
    }

    // --- misc one-liners --------------------------------------------------

    fn on_ignored_scripts(&mut self, log: &IgnoredScriptsLog) {
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
        self.push_block(format!(
            "Ignored build scripts: {list}.\nRun \"pnpm approve-builds\" to pick which dependencies should be allowed to run scripts.",
        ));
    }

    fn on_config_deps(&mut self, log: &InstallingConfigDepsLog) {
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

    fn on_lockfile_verification(&mut self, message: &LockfileVerificationMessage) {
        let msg = match message {
            LockfileVerificationMessage::Cached { lockfile_path, .. } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} passes supply-chain policies (previously verified)",
                    self.colors.green("✓"),
                )
            }
            LockfileVerificationMessage::Started { entries, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Verifying lockfile{path} against supply-chain policies ({})...",
                    self.colors.cyan("?"),
                    entries_label(*entries),
                )
            }
            LockfileVerificationMessage::Done { entries, elapsed_ms, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} passes supply-chain policies ({} in {})",
                    self.colors.green("✓"),
                    entries_label(*entries),
                    pretty_ms(u128::from(*elapsed_ms)),
                )
            }
            LockfileVerificationMessage::Failed { entries, elapsed_ms, lockfile_path } => {
                let path = self.lockfile_path_suffix(lockfile_path.as_deref());
                format!(
                    "{} Lockfile{path} failed supply-chain policy check ({} in {})",
                    self.colors.red("✗"),
                    entries_label(*entries),
                    pretty_ms(u128::from(*elapsed_ms)),
                )
            }
        };
        let mut slot = std::mem::take(&mut self.lockfile_verification_slot);
        self.frame.emit(&mut slot, msg, false);
        self.lockfile_verification_slot = slot;
    }

    fn lockfile_path_suffix(&self, lockfile_path: Option<&str>) -> String {
        let Some(path) = lockfile_path else { return String::new() };
        let from_expected = relative(&self.cwd, path);
        let is_direct_child = !from_expected.contains('/') && !from_expected.starts_with("..");
        if is_direct_child {
            return String::new();
        }
        format!(" at {}", normalize(&relative(&self.cwd, path)))
    }

    fn on_request_retry(&mut self, log: &RequestRetryLog) {
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

    fn on_pnpm(&mut self, level: LogLevel, message: &str, prefix: &str) {
        match level {
            LogLevel::Debug => {}
            LogLevel::Warn => self.push_warning(message),
            LogLevel::Error => self.push_block(message.to_string()),
            LogLevel::Info => {
                if prefix.is_empty() || prefix == self.cwd {
                    self.push_block(message.to_string());
                }
            }
        }
    }

    fn on_execution_time(&mut self, log: &ExecutionTimeLog) {
        let elapsed = log.ended_at.saturating_sub(log.started_at);
        let msg =
            format!("Done in {} using pnpm v{}", pretty_ms(elapsed), crate::package_version());
        let mut slot = std::mem::take(&mut self.exec_slot);
        self.frame.emit(&mut slot, msg, true);
        self.exec_slot = slot;
    }

    /// A warning, honoring pnpm's "only show the first
    /// [`MAX_SHOWN_WARNINGS`], then collapse the rest into a count" rule.
    fn push_warning(&mut self, message: &str) {
        self.warnings_counter += 1;
        if self.append_only || self.warnings_counter <= MAX_SHOWN_WARNINGS {
            self.push_block(format!("{} {message}", self.colors.warn_label()));
            return;
        }
        let extra = self.warnings_counter - MAX_SHOWN_WARNINGS;
        let msg = format!("{} {extra} other warnings", self.colors.warn_label());
        let mut slot = std::mem::take(&mut self.collapsed_warn_slot);
        self.frame.emit(&mut slot, msg, false);
        self.collapsed_warn_slot = slot;
    }

    fn push_block(&mut self, message: String) {
        let mut slot = BlockSlot::default();
        self.frame.emit(&mut slot, message, false);
    }
}

/// Strip ASCII control characters (C0 range 0x00–0x1F and DEL 0x7F)
/// from an override selector before rendering, so a crafted key
/// cannot inject/spoof terminal output.
fn sanitize_override_selector(selector: &str) -> String {
    selector.chars().filter(|ch| !ch.is_control()).collect()
}

fn diff_key(kind: DepKind) -> &'static str {
    match kind {
        DepKind::Prod => "prod",
        DepKind::Optional => "optional",
        DepKind::Peer => "peer",
        DepKind::Dev => "dev",
        DepKind::NodeModulesOnly => "nodeModulesOnly",
    }
}

type RootFields =
    (bool, DepKind, String, Option<String>, Option<String>, Option<String>, Option<String>);

fn added_fields(added: &AddedRoot) -> RootFields {
    (
        true,
        DepKind::from_dependency_type(added.dependency_type),
        added.name.clone(),
        added.version.clone().or_else(|| added.id.clone()),
        Some(added.real_name.clone()),
        added.linked_from.clone(),
        added.latest.clone(),
    )
}

fn removed_fields(removed: &RemovedRoot) -> RootFields {
    (
        false,
        DepKind::from_dependency_type(removed.dependency_type),
        removed.name.clone(),
        removed.version.clone(),
        None,
        None,
        None,
    )
}

fn lifecycle_ids(message: &LifecycleMessage) -> (&str, &str, &str) {
    match message {
        LifecycleMessage::Script { stage, dep_path, wd, .. }
        | LifecycleMessage::Stdio { stage, dep_path, wd, .. }
        | LifecycleMessage::Exit { stage, dep_path, wd, .. } => (stage, dep_path, wd),
    }
}

fn entries_label(entries: u64) -> String {
    if entries == 1 { "1 entry".to_string() } else { format!("{entries} entries") }
}

fn remove_optional_from_prod(manifest: &Value) -> Value {
    let mut manifest = manifest.clone();
    let optional: Vec<String> = manifest
        .get("optionalDependencies")
        .and_then(Value::as_object)
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(deps) = manifest.get_mut("dependencies").and_then(Value::as_object_mut) {
        for name in optional {
            deps.remove(&name);
        }
    }
    manifest
}

fn manifest_dep_versions(manifest: &Value, prop: &str) -> HashMap<String, String> {
    manifest
        .get(prop)
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}
