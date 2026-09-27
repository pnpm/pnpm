use super::{Lockfile, PnprLink, Reporter, State, pnpr_lockfile_dir};
use pnpm_hooks::PnpmfileHooks;
use pnpm_network::redact_url_for_display;
use std::sync::Arc;

/// The pnpr server an install resolves through, and the pnpmfile hooks the
/// install runs around it.
pub(crate) struct PnprTarget<'a> {
    pub(crate) server: &'a str,
    pub(crate) pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
}

/// Stamp the pnpmfile fields a local resolution records onto a lockfile the
/// server produced. The server never sees the pnpmfile, so its lockfile
/// carries neither.
pub(super) async fn record_pnpmfile(
    hook: Option<&Arc<dyn PnpmfileHooks>>,
    lockfile: &mut Lockfile,
) -> miette::Result<()> {
    lockfile.pnpmfile_checksum = match hook {
        Some(hook) => hook.calculate_pnpmfile_checksum().await,
        None => None,
    };
    let untracked = pnpm_hooks::untracked_read_package_hook(hook).await
        .map_err(|error| miette::miette!("{error}"))?;
    lockfile.set_untracked_pnpmfile_read_package_hook(untracked);
    Ok(())
}

/// The configured pnpr server, unless the pnpmfile defines a hook that
/// shapes resolution. The server runs no pnpmfile, so such an install
/// resolves locally and warns that the server is not used
/// ([pnpm/pnpm#14460](https://github.com/pnpm/pnpm/issues/14460)).
pub(crate) async fn pnpr_target<'a, Reporter: self::Reporter>(
    state: &'a State,
    link: &PnprLink<'_>,
) -> miette::Result<Option<PnprTarget<'a>>> {
    let Some(server) = state.config.pnpr_server.as_deref() else {
        return Ok(None);
    };
    let pnpmfile_hook = load_pnpmfile(state, pnpr_lockfile_dir(state, link))?;
    if let Some(hook) = pnpmfile_hook.as_ref()
        && let Some(unsupported) = resolution_hook(hook.as_ref()).await?
    {
        let server = redact_url_for_display(server);
        pnpm_reporter::emit_global_warning::<Reporter>(&format!(
            "Resolving dependencies locally because the pnpr server at {server} cannot run the pnpmfile's {unsupported}",
        ));
        return Ok(None);
    }
    Ok(Some(PnprTarget { server, pnpmfile_hook }))
}

/// The pnpmfile hooks this install runs, unless the run disabled them.
fn load_pnpmfile(
    state: &State,
    lockfile_dir: &std::path::Path,
) -> miette::Result<Option<Arc<dyn PnpmfileHooks>>> {
    if state.config.ignore_pnpmfile {
        return Ok(None);
    }
    pnpm_hooks::finder::load_pnpmfiles(
        lockfile_dir,
        pnpm_package_manager::pnpmfile_selection(state.config),
    )
    .map_err(|error| miette::miette!(code = "ERR_PNPM_PNPMFILE_NOT_FOUND", "{error}"))
}

/// The first pnpmfile hook that changes what resolution produces, named
/// for the warning.
async fn resolution_hook(hook: &dyn PnpmfileHooks) -> miette::Result<Option<&'static str>> {
    let hook_error = |error: pnpm_hooks::HookError| miette::miette!("{error}");
    if hook.has_read_package().await.map_err(hook_error)? {
        return Ok(Some(r#""readPackage" hook"#));
    }
    if hook.has_after_all_resolved().await.map_err(hook_error)? {
        return Ok(Some(r#""afterAllResolved" hook"#));
    }
    if !hook
        .get_custom_resolvers()
        .await
        .map_err(hook_error)?
        .is_empty()
    {
        return Ok(Some("custom resolvers"));
    }
    Ok(None)
}
