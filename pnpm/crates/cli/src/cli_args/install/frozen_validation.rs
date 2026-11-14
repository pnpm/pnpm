use super::{
    DryRunIncompatibleWithPnpr, InstallArgs, InstallFamilySelection, pnpr_config_validation,
    uses_pnpr_server, workspace_install_selection,
};
use crate::State;
use miette::Context;

impl InstallArgs {
    /// Validate frozen lockfile presence, manifest satisfaction, and pnpr
    /// configuration compatibility before `pnpm ci` cleans `node_modules`.
    pub(crate) async fn validate_frozen<Reporter: super::Reporter + 'static>(
        &self,
        state: &State,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
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
        install
            .validate_frozen(selection.map(workspace_install_selection))
            .await
            .wrap_err("validating frozen lockfile")
    }
}
