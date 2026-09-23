use std::{io, path::PathBuf};

/// The executable that re-invokes the running pnpm as `pnpm`.
///
/// On Windows, `pnpx.exe` and `pnx.exe` are hardlinks of the pnpm binary, and
/// the name it is launched through is what makes it run as `pnpm dlx`. A child
/// started from [`std::env::current_exe`] under one of those names would get
/// `dlx` prepended to its arguments, so the `pnpm` executable linked beside the
/// alias is returned instead.
pub fn current_pnpm_exe() -> io::Result<PathBuf> {
    std::env::current_exe().map(pnpm_exe_beside)
}

/// Whether `exe_stem`, an executable's file name without its extension, is one
/// of the `pnpx` and `pnx` aliases that run pnpm as `pnpm dlx`.
#[must_use]
pub fn is_pnpx_alias(exe_stem: &str) -> bool {
    exe_stem.eq_ignore_ascii_case("pnpx") || exe_stem.eq_ignore_ascii_case("pnx")
}

fn pnpm_exe_beside(exe: PathBuf) -> PathBuf {
    if !exe
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(is_pnpx_alias)
    {
        return exe;
    }
    let pnpm = exe.with_file_name("pnpm");
    match exe.extension() {
        Some(extension) => pnpm.with_extension(extension),
        None => pnpm,
    }
}

#[cfg(test)]
mod tests;
