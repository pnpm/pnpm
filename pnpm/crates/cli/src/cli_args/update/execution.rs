use super::{
    Config, Context, DependencyGroup, InstallFamilySelection, Matcher, Path, Reporter, State,
    Update, UpdateArgs, build_workspace_packages_map, github_actions, recursive,
};

fn manifest_root(manifest: &pnpm_package_manifest::PackageManifest) -> std::path::PathBuf {
    manifest.path().parent().expect("manifest path always has a parent directory").to_path_buf()
}

/// The matcher for the workflow selectors, when this run updates
/// workflow files at all.
fn loaded_lockfile(
    lockfile: &pnpm_lockfile::LazyLockfile,
) -> miette::Result<Option<&pnpm_lockfile::Lockfile>> {
    lockfile.get().map_err(|err| miette::Report::new(err).wrap_err("load the lockfile"))
}

/// The workspace's packages when the update runs inside one.
fn discovered_workspace_packages(
    workspace_root: Option<&Path>,
    config: &Config,
) -> miette::Result<Option<pnpm_resolving_resolver_base::WorkspacePackages>> {
    workspace_root
        .map(|workspace_root| {
            recursive::discover_workspace_projects(workspace_root, config)
                .map(|(projects, _)| build_workspace_packages_map(Some(&projects)))
        })
        .transpose()
        .map(Option::flatten)
}

fn actions_selector_matcher(update_actions: bool, selectors: &[String]) -> Option<Matcher> {
    update_actions.then(|| github_actions::selector_matcher(selectors)).flatten()
}

fn filter_package_selectors(packages: &[String], include_github_actions: bool) -> Vec<String> {
    packages
        .iter()
        .filter(|selector| !include_github_actions || !github_actions::is_selector(selector))
        .cloned()
        .collect()
}

struct UpdateInputs {
    include_direct: Vec<DependencyGroup>,
    update_actions: bool,
    lockfile_path: std::path::PathBuf,
    workspace_packages: Option<pnpm_resolving_resolver_base::WorkspacePackages>,
    actions_root: std::path::PathBuf,
}

fn update_actions_root(
    state: &State,
    selection: Option<&InstallFamilySelection>,
) -> std::path::PathBuf {
    selection.map_or_else(
        || state.config.workspace_dir.clone().unwrap_or_else(|| manifest_root(&state.manifest)),
        |selection| selection.workspace_root.clone(),
    )
}

impl UpdateArgs {
    pub(super) async fn run_inner<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: Option<InstallFamilySelection>,
    ) -> miette::Result<()> {
        self.check_patches_options()?;
        state.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        let workspace_root = self.check_workspace_option(state.config.workspace_dir.as_deref())?;
        let include_direct = self.dependency_options.include_direct();
        let update_actions = self.should_update_github_actions(state.config, &include_direct);
        let lockfile_path = state.lockfile_path();
        if let Some(pnpr_server) =
            self.delegated_pnpr_server(state.config, update_actions, &include_direct)
        {
            return self
                .run_patch_refresh::<Reporter>(
                    &state,
                    selection.as_ref(),
                    pnpr_server,
                    &lockfile_path,
                )
                .await;
        }
        let workspace_packages = match &selection {
            Some(selection) => {
                workspace_root.and_then(|_| build_workspace_packages_map(Some(&selection.projects)))
            }
            None => discovered_workspace_packages(workspace_root, state.config)?,
        };
        let actions_root = update_actions_root(&state, selection.as_ref());
        self.run_local::<Reporter>(
            state,
            selection,
            &UpdateInputs {
                include_direct,
                update_actions,
                lockfile_path,
                workspace_packages,
                actions_root,
            },
        )
        .await
    }

    async fn run_patch_refresh<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        selection: Option<&InstallFamilySelection>,
        pnpr_server: &str,
        lockfile_path: &Path,
    ) -> miette::Result<()> {
        let link = self.pnpr_patch_link(state, lockfile_path);
        match selection {
            Some(selection) => {
                super::super::install::install_selected_via_pnpr::<Reporter>(
                    state,
                    pnpr_server,
                    selection,
                    link,
                )
                .await
            }
            None => {
                super::super::install::install_via_pnpr::<Reporter>(state, pnpr_server, link).await
            }
        }
    }

    async fn run_local<Reporter: self::Reporter + 'static>(
        &self,
        mut state: State,
        mut selection: Option<InstallFamilySelection>,
        inputs: &UpdateInputs,
    ) -> miette::Result<()> {
        let Some(packages) =
            self.select_local_packages::<Reporter>(&state, selection.as_ref(), inputs).await?
        else {
            return Ok(());
        };
        let action_matcher = if self.interactive {
            github_actions::selector_matcher(&packages)
        } else {
            actions_selector_matcher(inputs.update_actions, &self.packages)
        };
        let packages = filter_package_selectors(&packages, inputs.update_actions);
        if self.updates_packages(&packages) {
            let update = self.prepare_update(&mut state, inputs, &packages)?;
            match &mut selection {
                Some(selection) => {
                    update.run_selected::<Reporter>(selection.selected_projects()).await
                }
                None => update.run::<Reporter>().await,
            }
            .wrap_err("updating dependencies")?;
        }
        self.update_github_actions::<Reporter>(
            inputs.update_actions,
            &inputs.actions_root,
            action_matcher.as_ref(),
            state.config,
        )
        .await
    }

    async fn select_local_packages<Reporter: self::Reporter + 'static>(
        &self,
        state: &State,
        selection: Option<&InstallFamilySelection>,
        inputs: &UpdateInputs,
    ) -> miette::Result<Option<Vec<String>>> {
        let packages = filter_package_selectors(&self.packages, inputs.update_actions);
        if !self.interactive && !self.packages.is_empty() && packages.is_empty() {
            self.update_github_actions::<Reporter>(
                inputs.update_actions,
                &inputs.actions_root,
                actions_selector_matcher(inputs.update_actions, &self.packages).as_ref(),
                state.config,
            )
            .await?;
            return Ok(None);
        }
        let lockfile = loaded_lockfile(&state.lockfile)?;
        let prompt = self.interactive_options(&inputs.include_direct, inputs.update_actions);
        match selection {
            Some(selection) => {
                self.prompted_or_given_for_projects::<Reporter>(
                    state,
                    selection,
                    lockfile,
                    &inputs.actions_root,
                    prompt,
                    packages,
                )
                .await
            }
            None => {
                self.prompted_or_given::<Reporter>(
                    state,
                    lockfile,
                    &inputs.actions_root,
                    prompt,
                    packages,
                )
                .await
            }
        }
    }

    fn prepare_update<'a>(
        &self,
        state: &'a mut State,
        inputs: &'a UpdateInputs,
        packages: &'a [String],
    ) -> miette::Result<Update<'a>> {
        Ok(Update {
            tarball_mem_cache: std::sync::Arc::clone(&state.tarball_mem_cache),
            resolved_packages: &state.resolved_packages,
            http_client: &state.http_client,
            http_client_arc: std::sync::Arc::clone(&state.http_client),
            config: state.config,
            manifest: &mut state.manifest,
            lockfile: loaded_lockfile(&state.lockfile)?,
            lockfile_path: Some(&inputs.lockfile_path),
            packages,
            latest: self.latest,
            patches: self.patches,
            save_exact: self.save_exact || state.config.save_exact,
            save: !self.no_save,
            include_direct: inputs.include_direct.clone(),
            depth: self.depth.unwrap_or(usize::MAX),
            workspace_packages: inputs.workspace_packages.as_ref(),
            supported_architectures: self
                .supported_architectures
                .apply_to(state.config.supported_architectures.clone()),
            lockfile_only: self.lockfile_only,
            resolution_observer: None,
        })
    }

    /// Run the GitHub Actions half of the update, if this run covers it.
    async fn update_github_actions<Reporter: self::Reporter + 'static>(
        &self,
        update_actions: bool,
        actions_root: &Path,
        matcher: Option<&Matcher>,
        config: &Config,
    ) -> miette::Result<()> {
        if !update_actions {
            return Ok(());
        }
        github_actions::update::<Reporter>(
            actions_root,
            self.latest,
            matcher,
            config.update_config.github_actions_server.as_deref(),
        )
        .await?;
        Ok(())
    }

    /// Whether the package half of the update runs. An interactive run that
    /// ended with no package selected updates only workflow files.
    fn updates_packages(&self, package_selectors: &[String]) -> bool {
        !self.interactive || !package_selectors.is_empty()
    }

    /// The pnpr server this run may delegate to, when it has nothing to do
    /// beyond refreshing patches.
    fn delegated_pnpr_server<'config>(
        &self,
        config: &'config Config,
        update_actions: bool,
        include_direct: &[DependencyGroup],
    ) -> Option<&'config str> {
        self.can_delegate_patch_refresh(update_actions, include_direct)
            .then_some(config.pnpr_server.as_deref())
            .flatten()
    }
}
