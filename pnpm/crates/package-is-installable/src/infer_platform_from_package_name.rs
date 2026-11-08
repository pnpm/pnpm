//! Infers a package's supported platforms from the tokens of its name.

use crate::{
    check_platform::{WantedPlatform, WantedPlatformRef},
    node_platform_names::{cpu_for_token, os_for_token},
};

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
    let name_without_scope = name
        .find('/')
        .map_or(name, |idx| &name[idx + 1..]);
    let lowercase = name_without_scope.to_lowercase();
    let tokens: Vec<&str> = lowercase
        .split(['-', '_', '.'])
        .collect();
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
/// A package that declares none of the fields is inferred only when its name
/// has an os token and a cpu or libc token, so a lone `windows` or `arm`
/// segment does not mark it as platform-specific.
///
/// Returns `None` when the declared fields stand as-is. The `optional` gate
/// stays at the call site.
/// See <https://github.com/pnpm/pnpm/issues/11702>.
pub fn inferred_platform(name: &str, declared: WantedPlatformRef<'_>) -> Option<WantedPlatform> {
    if declared.os.is_some() && declared.cpu.is_some() && declared.libc.is_some() {
        return None;
    }
    let inferred = infer_platform_from_package_name(name)?;
    let declares_platform =
        declared.os.is_some() || declared.cpu.is_some() || declared.libc.is_some();
    let names_a_binary_target =
        inferred.os.is_some() && (inferred.cpu.is_some() || inferred.libc.is_some());
    if !declares_platform && !names_a_binary_target {
        return None;
    }
    Some(WantedPlatform {
        os: declared.os.map(<[String]>::to_vec).or(inferred.os),
        cpu: declared.cpu.map(<[String]>::to_vec).or(inferred.cpu),
        libc: declared.libc.map(<[String]>::to_vec).or(inferred.libc),
    })
}
