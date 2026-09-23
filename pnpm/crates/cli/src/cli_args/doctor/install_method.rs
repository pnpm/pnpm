use super::CheckResult;
use crate::cli_args::shadowing_pnpm::find_shadowing_pnpm;
use pnpm_config::Config;

/// Report how the running pnpm is managed, and whether `pnpm` on `PATH` is
/// the one `self-update` and `setup` install: a copy from another installer
/// ahead of the global bin directory is why an update "does not take".
pub(super) fn check_install_method(config: &Config) -> CheckResult {
    let title = "Install method";
    if std::env::var_os("COREPACK_ROOT").is_some() {
        return CheckResult::warn(
            title,
            "pnpm, run by Corepack",
            r#"Corepack manages the pnpm version itself; "pnpm self-update" is unavailable under it."#,
        );
    }
    if let Some(global_bin) = config.global_bin.as_deref()
        && let Some(shadowing) =
            find_shadowing_pnpm(global_bin, std::env::var_os("PATH").as_deref())
    {
        return CheckResult::warn(
            title,
            format!(
                r#""pnpm" on PATH is {}, not the pnpm in {}"#,
                shadowing.executable.display(),
                global_bin.display(),
            ),
            shadowing.warning(global_bin),
        );
    }
    CheckResult::pass(title, "pnpm")
}
