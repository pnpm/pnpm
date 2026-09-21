use super::{
    IgnoredAny, IndexMap, LoadWorkspaceYamlError, NAMED_UNRECOGNIZED_TASK_SETTINGS, Path,
    RemoteSideEffectsCacheSettings, SCHEMA_DIRECTIVE_KEY, SideEffectsCacheSetting, TaskSettings,
    UnrecognizedTaskSettings, WorkspaceKeyIssues, WorkspaceSettings, is_camel_case,
    is_known_setting_key, is_refused_by_a_project_manifest, registries,
};

impl WorkspaceSettings {
    /// Reject a `registries` map pnpm would read as something other than what
    /// it says. See [`registries::validate`] for the rules.
    ///
    /// Checked after parsing rather than in a `Deserialize` impl on purpose:
    /// `serde_saphyr` renders the offending source line verbatim under its
    /// errors, so rejecting at parse time would print the very credential
    /// being rejected into the terminal and any CI log.
    pub(super) fn validate_registries(&self) -> Result<(), LoadWorkspaceYamlError> {
        let Some(entries) = self.registries.as_ref() else { return Ok(()) };
        registries::validate(entries)
    }

    /// The `tasks` section feeds the task-graph builder of `pnpm -r run`,
    /// which reads it without further checks — a malformed entry has to be
    /// rejected here rather than surface as a scheduling bug far from the
    /// setting that produced it.
    pub(super) fn validate_tasks(&self) -> Result<(), LoadWorkspaceYamlError> {
        let Some(tasks) = self.tasks.as_ref() else { return Ok(()) };
        for (task, settings) in tasks {
            Self::validate_task(task, settings)?;
        }
        Ok(())
    }

    /// One task's settings: a non-positive concurrency, a group name that
    /// cannot be a slot directory, a non-integer `priority`, or an empty
    /// `dependsOn` entry is a hard error. A field this version does not
    /// read is not — see [`Self::take_unknown_task_settings`].
    pub(super) fn validate_task(
        task: &str,
        settings: &TaskSettings,
    ) -> Result<(), LoadWorkspaceYamlError> {
        if let Some(error) = invalid_task_concurrency(task, settings) {
            return Err(error);
        }
        if let Some(priority) = settings.invalid_priority.as_ref() {
            return Err(LoadWorkspaceYamlError::InvalidTaskPriority {
                task: task.to_string(),
                priority: priority.to_string(),
            });
        }
        if let Some(group) = settings.concurrency_group
            .as_deref()
            .filter(|group| !is_valid_concurrency_group_name(group))
        {
            return Err(LoadWorkspaceYamlError::InvalidTaskConcurrencyGroup {
                task: task.to_string(),
                group: group.to_string(),
            });
        }
        for entry in settings.depends_on.iter().flatten() {
            if entry.is_empty() || entry == "^" {
                return Err(LoadWorkspaceYamlError::EmptyTaskDependsOnEntry {
                    task: task.to_string(),
                    entry: entry.clone(),
                });
            }
        }
        Ok(())
    }

    /// Take the fields of the `tasks` entries this version of pnpm does not
    /// read, as the paths that name them. A project may pin a pnpm that
    /// reads them, so they are reported rather than refused.
    ///
    /// Taking them is what makes the report true: a field left in place
    /// would reach the configuration record and be printed back by
    /// `pnpm config`, as a setting pnpm had said it ignored. An entry that
    /// carried nothing else goes with them, so a setting this pnpm does not
    /// read cannot quietly reorder its tasks.
    fn take_unknown_task_settings(&mut self) -> UnrecognizedTaskSettings {
        let mut report = UnrecognizedTaskSettings::default();
        let Some(tasks) = self.tasks.as_mut() else { return report };
        tasks.retain(|task, settings| {
            if settings.unknown.is_empty() {
                return true;
            }
            report.total += settings.unknown.len();
            let named = settings.unknown
                .keys()
                .take(NAMED_UNRECOGNIZED_TASK_SETTINGS.saturating_sub(report.named.len()));
            report.named.extend(named.map(|field| format!("tasks['{task}'].{field}")));
            settings.unknown.clear();
            // An entry left with nothing declares nothing, and a task with no
            // entry is the one that keeps the default `^<name>` ordering. A
            // setting the running pnpm cannot read must not reorder tasks.
            *settings != TaskSettings::default()
        });
        report
    }

    /// The `pipelines` section feeds `pnpm pipeline`'s task requests, which
    /// reads it without further checks.
    pub(super) fn validate_pipelines(&self) -> Result<(), LoadWorkspaceYamlError> {
        for (pipeline, task_names) in self.pipelines.iter().flatten() {
            if task_names.iter().any(String::is_empty) {
                return Err(LoadWorkspaceYamlError::EmptyPipelineTaskName {
                    pipeline: pipeline.clone(),
                });
            }
        }
        Ok(())
    }

    /// Reject every remote side-effects field a committed file may not set.
    ///
    /// A workspace declares which organization and packages are eligible and
    /// nothing else: the rest describes the act of signing — which key signs,
    /// what provenance the signature attests, and whether to publish at all —
    /// so it belongs to the machine holding the key. Letting a repository set
    /// `publish` would turn a key the machine holds for its own builds into a
    /// signing oracle any clone could aim at a registry of its choosing.
    ///
    /// Checked after parsing rather than through `deny_unknown_fields` because
    /// the same struct also parses the global config yaml, where every field is
    /// legitimate.
    pub(super) fn reject_repo_controlled_trust_material(
        &self,
        path: &Path,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let canonical = match self.side_effects_cache.as_ref() {
            Some(SideEffectsCacheSetting::Settings(settings)) => settings.remote.as_ref(),
            _ => None,
        };
        // The message names the spelling the file actually used, since telling
        // someone to move `remoteSideEffectsCache.privateKey` out of a file
        // that says `sideEffectsCache.remote.privateKey` sends them looking
        // for a key that is not there.
        for (setting, prefix) in [
            (canonical, "sideEffectsCache.remote"),
            (self.remote_side_effects_cache.as_ref(), "remoteSideEffectsCache"),
        ] {
            let Some(settings) = setting else { continue };
            Self::reject_machine_only_fields(settings, prefix, path)?;
        }
        Ok(())
    }

    pub(super) fn reject_machine_only_fields(
        settings: &RemoteSideEffectsCacheSettings,
        prefix: &'static str,
        path: &Path,
    ) -> Result<(), LoadWorkspaceYamlError> {
        let machine_only = [
            ("publish", settings.publish.is_some()),
            ("keyId", settings.key_id.is_some()),
            ("builderId", settings.builder_id.is_some()),
            ("imageDigest", settings.image_digest.is_some()),
            ("architectureBaseline", settings.architecture_baseline.is_some()),
            ("buildEnv", settings.build_env.is_some()),
            ("trustedKeys", settings.trusted_keys.is_some()),
            ("privateKey", settings.private_key.is_some()),
        ];
        let Some((field, _)) = machine_only.into_iter().find(|(_, is_set)| *is_set) else {
            return Ok(());
        };
        Err(LoadWorkspaceYamlError::WorkspaceRemoteSideEffectsTrust {
            path: path.to_path_buf(),
            prefix,
            field,
        })
    }

    /// Bucket the file's keys that set nothing into [`Self::key_issues`],
    /// under the project-file rules: refused values, keys naming no setting
    /// any supported pnpm reads, kebab-case spellings of known settings, and
    /// the `tasks` entries' own unknown fields.
    /// Reporting is the caller's job — how severe an unrecognized key is
    /// depends on whether the running pnpm is the project's pinned version,
    /// which only the CLI layer knows.
    pub fn collect_key_issues(&mut self, text: &str) {
        let unrecognized_task_settings = self.take_unknown_task_settings();
        let mut issues = Self::top_level_key_issues(text);
        issues.unrecognized_task_settings = unrecognized_task_settings;
        self.key_issues = issues;
    }

    /// The top-level keys of `text` that set nothing, in the three buckets
    /// they are reported under.
    fn top_level_key_issues(text: &str) -> WorkspaceKeyIssues {
        let mut issues = WorkspaceKeyIssues::default();
        if !Self::may_have_key_issues(text) {
            return issues;
        }
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return issues;
        };
        for key in document
            .iter()
            .filter(|(_, value)| value.is_some())
            .map(|(key, _)| key)
        {
            if key == SCHEMA_DIRECTIVE_KEY {
                continue;
            }
            if is_refused_by_a_project_manifest(key) {
                issues.refused.push(key.clone());
            } else if !is_known_setting_key(key) {
                issues.unrecognized.push(key.clone());
            } else if !is_camel_case(key) {
                issues.non_camel_case.push(key.clone());
            }
        }
        issues
    }

    /// Whether `text` may carry a key [`WorkspaceSettings::collect_key_issues`]
    /// would report, answered without parsing the file a second time.
    ///
    /// Serde keeps no record of the keys it dropped, so collecting them means
    /// re-reading the document — which costs as much as the parse that
    /// produced the settings, on every command, to find nothing in the
    /// overwhelmingly common case of a file that is simply correct.
    ///
    /// The top-level keys are the least indented lines of the document, since
    /// everything a key nests under it is indented further; the root mapping
    /// may itself be indented, so what marks a top-level key is the smallest
    /// indentation the file uses rather than column zero. A line this cannot
    /// measure or classify counts as one to look at, so the answer errs only
    /// towards re-reading, never towards missing a key.
    pub(super) fn may_have_key_issues(text: &str) -> bool {
        let content_lines = text
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            });
        let mut root_indent = usize::MAX;
        for line in content_lines.clone() {
            let indent = line.len() - line.trim_start().len();
            // YAML forbids a tab as indentation, so a file that uses one is
            // not worth measuring against.
            if line[..indent]
                .bytes()
                .any(|byte| byte != b' ')
            {
                return true;
            }
            root_indent = root_indent.min(indent);
        }
        if root_indent == usize::MAX {
            return false;
        }
        content_lines
            .filter(|line| line.len() - line.trim_start().len() == root_indent)
            .any(|line| {
                let Some((key, _)) = line.trim_start().split_once(':') else { return true };
                let key = key.trim_end();
                key != SCHEMA_DIRECTIVE_KEY
                    && (!is_camel_case(key)
                        || !is_known_setting_key(key)
                        || is_refused_by_a_project_manifest(key))
            })
    }
}

fn invalid_task_concurrency(task: &str, settings: &TaskSettings) -> Option<LoadWorkspaceYamlError> {
    let concurrency = settings.concurrency
        .filter(|concurrency| *concurrency < 1)
        .map(|concurrency| concurrency.to_string())
        .or_else(|| settings.invalid_concurrency.as_ref().map(ToString::to_string))?;
    Some(LoadWorkspaceYamlError::InvalidTaskConcurrency { task: task.to_string(), concurrency })
}

/// A group name becomes the name of the group's slot directory under the
/// state directory, so it is held to a portable file name that cannot
/// leave that directory: the Windows device names are refused whatever
/// follows their first dot, and a trailing dot would be dropped there.
fn is_valid_concurrency_group_name(group: &str) -> bool {
    !group.is_empty()
        && !group.ends_with('.')
        && group
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-'))
        && !is_windows_device_name(
            group
                .split('.')
                .next()
                .unwrap_or_default(),
        )
}

fn is_windows_device_name(stem: &str) -> bool {
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|char: char| char.is_ascii_digit() && char != '0'))
}
