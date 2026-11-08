//! The Node.js `process.platform` and `process.arch` values pnpm knows, and
//! the other spellings a package name or a platform may use for them.

pub(crate) fn os_for_token(token: &str) -> Option<&'static str> {
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

pub(crate) fn cpu_for_token(token: &str) -> Option<&'static str> {
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

/// Whether `token` is a Node.js `process.platform` value pnpm knows.
pub(crate) fn is_node_os(token: &str) -> bool {
    os_for_token(token) == Some(token)
}

/// Whether `token` is a Node.js `process.arch` value pnpm knows, of a
/// machine rather than of `wasm32`.
pub(crate) fn is_node_cpu(token: &str) -> bool {
    token != "wasm32" && cpu_for_token(token) == Some(token)
}
