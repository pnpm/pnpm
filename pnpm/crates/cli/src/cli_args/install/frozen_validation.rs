use super::{
    DryRunIncompatibleWithPnpr, InstallArgs, InstallFamilySelection, pnpr_config_validation,
    uses_pnpr_server, workspace_install_selection,
};
use crate::State;
use miette::Context;
use pnpm_package_manager::{FrozenVirtualStoreClean, PreparedFrozenInstall};

impl InstallArgs {
    /// Validate pre-clean invariants and prepare frozen-lockfile hook/verification
    /// artifacts before `pnpm ci` cleans `node_modules`.
    pub(crate) async fn prepare_for_ci<Reporter: super::Reporter + 'static>(
        &self,
        state: &State,
        selection: Option<&InstallFamilySelection>,
        virtual_store_clean: FrozenVirtualStoreClean,
    ) -> miette::Result<Option<PreparedFrozenInstall>> {
        state.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
        if state.config.pnpr_server.is_some() && self.materialization.dry_run {
            return Err(DryRunIncompatibleWithPnpr.into());
        }
        let frozen_lockfile = self.resolve_frozen_lockfile(state)?;
        let lockfile_path = state.lockfile_path();
        let link = self.resolve_link_options(state.config, &lockfile_path, frozen_lockfile);
        if uses_pnpr_server(state, &link).await? {
            pnpr_config_validation::validate_pnpr_config(state.config, link.lockfile.frozen)?;
        }
        let install = self.build_local_install(state, link);
        let ws_selection = selection.map(workspace_install_selection);
        if self.lockfile.fix {
            install
                .validate_preflight(ws_selection)
                .await
                .wrap_err("validating install preflight")?;
            return Ok(None);
        }
        install
            .prepare_frozen::<Reporter>(ws_selection, virtual_store_clean)
            .await
            .map(Some)
            .wrap_err("validating frozen lockfile")
    }
}
