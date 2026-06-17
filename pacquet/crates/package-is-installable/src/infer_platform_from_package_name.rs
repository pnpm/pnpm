//! Port of `inferPlatformFromPackageName.ts` from
//! <https://github.com/pnpm/pnpm/blob/34875b2d7c/config/package-is-installable/src/inferPlatformFromPackageName.ts>.

use crate::check_platform::{WantedPlatform, WantedPlatformRef};

fn os_for_token(token: &str) -> Option<&'static str> {
    match token {
        "aix" => Some("aix"),
        "android" => Some("android"),
        "darwin" | "macos" | "osx" => Some("darwin"),
        "freebsd" => Some("freebsd"),
        "linux" => Some("linux"),
        "netbsd" => Some("netbsd"),
        "openbsd" => Some("openbsd"),
        "openharmony" => Some("openharmony"),
        "sunos" => Some("sunos"),
        "win32" | "windows" => Some("win32"),
        _ => None,
    }
}

fn cpu_for_token(token: &str) -> Option<&'static str> {
    match token {
        "arm" | "armv6" | "armv7" => Some("arm"),
        "arm64" | "aarch64" => Some("arm64"),
        "ia32" => Some("ia32"),
        "loong64" => Some("loong64"),
        "mips64el" => Some("mips64el"),
        "ppc64" | "ppc64le" => Some("ppc64"),
        "riscv64" => Some("riscv64"),
        "s390x" => Some("s390x"),
        "x64" | "amd64" => Some("x64"),
        "wasm32" => Some("wasm32"),
        _ => None,
    }
}

fn libc_for_token(token: &str) -> Option<&'static str> {
    match token {
        "glibc" | "gnu" | "gnueabihf" => Some("glibc"),
        "musl" | "musleabihf" => Some("musl"),
        _ => None,
    }
}

/// Infers the supported platforms of a package from the tokens of its name.
/// Platform-specific binary packages follow this naming convention, which is
/// the only platform signal left when their os/cpu/libc manifest fields are
/// absent.
pub fn infer_platform_from_package_name(name: &str) -> Option<WantedPlatform> {
    let name_without_scope = name.find('/').map_or(name, |idx| &name[idx + 1..]);
    let lowercase = name_without_scope.to_lowercase();
    let tokens: Vec<&str> = lowercase.split(['-', '_', '.']).collect();
    let os = pick_token_values(&tokens, os_for_token);
    let cpu = pick_token_values(&tokens, cpu_for_token);
    let libc = pick_token_values(&tokens, libc_for_token);
    if os.is_none() && cpu.is_none() && libc.is_none() {
        return None;
    }
    Some(WantedPlatform { os, cpu, libc })
}

fn pick_token_values(
    tokens: &[&str],
    value_for_token: fn(&str) -> Option<&'static str>,
) -> Option<Vec<String>> {
    let mut values: Vec<String> = Vec::new();
    for token in tokens {
        if let Some(value) = value_for_token(token)
            && !values.iter().any(|seen| seen == value)
        {
            values.push(value.to_string());
        }
    }
    (!values.is_empty()).then_some(values)
}

/// The platform fields of an optional dependency may be incomplete: some
/// registries strip os/cpu/libc (or just libc) from the metadata they serve,
/// and lockfile entries written from such metadata lack them too. For a
/// platform-specific binary the package name carries the same information.
///
/// Returns `None` when the declared fields stand as-is. The `optional` gate
/// stays at the call site, mirroring upstream's `effectivePlatform` at
/// <https://github.com/pnpm/pnpm/blob/34875b2d7c/config/package-is-installable/src/index.ts#L70-L96>.
/// See <https://github.com/pnpm/pnpm/issues/11702>.
pub fn inferred_platform(name: &str, declared: WantedPlatformRef<'_>) -> Option<WantedPlatform> {
    if declared.os.is_some() && declared.cpu.is_some() && declared.libc.is_some() {
        return None;
    }
    let inferred = infer_platform_from_package_name(name)?;
    let declares_platform =
        declared.os.is_some() || declared.cpu.is_some() || declared.libc.is_some();
    if !declares_platform && inferred.os.is_none() {
        return None;
    }
    Some(WantedPlatform {
        os: declared.os.map(<[String]>::to_vec).or(inferred.os),
        cpu: declared.cpu.map(<[String]>::to_vec).or(inferred.cpu),
        libc: declared.libc.map(<[String]>::to_vec).or(inferred.libc),
    })
}
