use super::{
    IgnoredAny, IndexMap, LoadWorkspaceYamlError, Path, RemoteSideEffectsCacheSettings,
    SCHEMA_DIRECTIVE_KEY, SideEffectsCacheSetting, TaskSettings, WorkspaceKeyIssues,
    WorkspaceSettings, is_camel_case, is_known_setting_key, is_refused_by_a_project_manifest,
    registries,
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

    /// One task's settings: an unrecognized field, a non-positive
    /// concurrency, or an empty `dependsOn` entry is a hard error.
    pub(super) fn validate_task(
        task: &str,
        settings: &TaskSettings,
    ) -> Result<(), LoadWorkspaceYamlError> {
        if let Some(field) = settings.unknown.keys().next() {
            return Err(LoadWorkspaceYamlError::UnknownTaskSettingField {
                task: task.to_string(),
                field: field.clone(),
            });
        }
        let concurrency = settings
            .concurrency
            .filter(|concurrency| *concurrency < 1)
            .map(|concurrency| concurrency.to_string())
            .or_else(|| settings.invalid_concurrency.as_ref().map(ToString::to_string));
        if let Some(concurrency) = concurrency {
            return Err(LoadWorkspaceYamlError::InvalidTaskConcurrency {
                task: task.to_string(),
                concurrency,
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
    /// any supported pnpm reads, and kebab-case spellings of known settings.
    /// Reporting is the caller's job — how severe an unrecognized key is
    /// depends on whether the running pnpm is the project's pinned version,
    /// which only the CLI layer knows.
    pub fn collect_key_issues(&mut self, text: &str) {
        if !Self::may_have_key_issues(text) {
            return;
        }
        let Ok(document) = serde_saphyr::from_str::<IndexMap<String, Option<IgnoredAny>>>(text)
        else {
            return;
        };
        let mut issues = WorkspaceKeyIssues::default();
        for key in document.iter().filter(|(_, value)| value.is_some()).map(|(key, _)| key) {
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
        self.key_issues = issues;
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
        let content_lines = text.lines().filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        });
        let mut root_indent = usize::MAX;
        for line in content_lines.clone() {
            let indent = line.len() - line.trim_start().len();
            // YAML forbids a tab as indentation, so a file that uses one is
            // not worth measuring against.
            if line[..indent].bytes().any(|byte| byte != b' ') {
                return true;
            }
            root_indent = root_indent.min(indent);
        }
        if root_indent == usize::MAX {
            return false;
        }
        content_lines.filter(|line| line.len() - line.trim_start().len() == root_indent).any(
            |line| {
                let Some((key, _)) = line.trim_start().split_once(':') else { return true };
                let key = key.trim_end();
                key != SCHEMA_DIRECTIVE_KEY
                    && (!is_camel_case(key)
                        || !is_known_setting_key(key)
                        || is_refused_by_a_project_manifest(key))
            },
        )
    }
}
