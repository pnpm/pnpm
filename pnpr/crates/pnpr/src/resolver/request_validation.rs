use axum::{http::StatusCode, response::Response};
use pnpm_lockfile::LockfileResolution;

use pnpr_route::RouteContext;

use super::{json_error, protocol::ResolveRequest};

/// Reject a request that would have pnpr fetch from an origin that is not on
/// the route allowlist (see [`RouteContext::allows_registry`]) — the
/// resolver's SSRF boundary, run before any server-side fetch. pnpr fetches
/// only from operator-configured registries, so a caller cannot point it at
/// cloud instance metadata, an internal service, or any other off-allowlist
/// host. Beyond the default/named registries this also covers every fetch a
/// *direct-URL dependency* would trigger: an `http(s)`/`git` dependency spec,
/// a catalog or package-extension entry, an override URL leaf, or an input
/// lockfile's tarball URL. A semver range or `npm:`/`workspace:`/`file:` alias
/// never hits the network, so it is ignored.
pub(super) fn reject_off_allowlist_fetches(
    request: &ResolveRequest,
    context: &RouteContext,
) -> Option<Response> {
    // Registries are fetch targets whatever their scheme. The registries the
    // request *declares* are deliberately not checked here: a client describes
    // its whole configuration, including scopes this resolve never reaches, and
    // one of those is refused at the fetch instead (`RouteHook::allows_fetch`)
    // so configuring a registry pnpr does not serve is not itself an error.
    let mut registries: Vec<&str> = Vec::new();
    if let Some(registry) = request.registry.as_deref() {
        registries.push(registry);
    }
    if let Some(off) = registries.into_iter().find(|registry| !context.allows_registry(registry)) {
        return Some(forbidden_off_allowlist(off));
    }

    // Direct-URL dependency specs and input-lockfile tarball URLs reach the
    // network only when they carry an http(s)/git URL.
    let mut url_specs: Vec<&str> = Vec::new();
    let projects = request.projects_normalized();
    for project in &projects {
        for map in
            [&project.dependencies, &project.dev_dependencies, &project.optional_dependencies]
        {
            url_specs.extend(map.values().map(String::as_str));
        }
    }
    if let Some(catalogs) = request.catalogs.as_ref() {
        url_specs
            .extend(catalogs.values().flat_map(|catalog| catalog.values()).map(String::as_str));
    }
    extend_package_extension_specs(request, &mut url_specs);
    if let Some(packages) =
        request.lockfile.as_ref().and_then(|lockfile| lockfile.packages.as_ref())
    {
        for package in packages.values() {
            if let LockfileResolution::Tarball(resolution) = &package.resolution {
                url_specs.push(resolution.tarball.as_str());
            }
        }
    }
    if let Some(off) = url_specs.into_iter().find(|spec| fetch_is_off_allowlist(spec, context)) {
        return Some(forbidden_off_allowlist(off));
    }

    // Override leaves can themselves be direct-URL specs.
    if let Some(off) = request
        .overrides
        .as_ref()
        .and_then(|overrides| first_off_allowlist_override(overrides, context))
    {
        return Some(forbidden_off_allowlist(&off));
    }

    None
}

/// Whether `spec` would trigger a server-side fetch to an origin that is not on
/// the allowlist. Covers any `scheme://host` URL (an `http(s)` tarball and
/// every git transport — `git`/`ssh`/`rsync`/`ftp`/`file`/... — with a `git+`
/// prefix stripped) and scp-style git remotes (`[user@]host:path`), which
/// pacquet routes to the ssh git resolver. Specs that never reach the network —
/// semver ranges, `npm:`/`workspace:`/`file:`/`link:` aliases (no `://`),
/// scoped names — return `false`.
fn fetch_is_off_allowlist(spec: &str, context: &RouteContext) -> bool {
    let url = spec.strip_prefix("git+").unwrap_or(spec);
    if url.contains("://") {
        // Gate by origin regardless of scheme: any transport that reaches a
        // host can be an SSRF vector (every git transport — git/ssh/rsync/ftp/
        // file/...), and a scheme with no allowlistable host (e.g. `file://`,
        // which would read a server-local path) nerf-darts to nothing and is
        // rejected.
        return !context.allows_registry(url);
    }
    // A scp-style git remote carries no scheme, so normalize its host to an
    // `ssh://host/` origin the allowlist can match (nerf-darting is
    // scheme-agnostic, so an operator allowlisting `https://host/` covers it).
    match scp_git_host(url) {
        Some(host) => !context.allows_registry(&format!("ssh://{host}/")),
        None => false,
    }
}

/// The host of a scp-style git remote (`[user@]host:path`), or `None`. The
/// distinguishing shape is a `user@host` authority before the first `:` with a
/// path after it — generalizing the `git@...` form pacquet's git resolver treats
/// as ssh. A protocol spec (`npm:...`, `file:...`) has no `@` in its authority, and
/// a `scheme://...` URL is handled before this is reached.
fn scp_git_host(spec: &str) -> Option<&str> {
    let (authority, path) = spec.split_once(':')?;
    if path.is_empty() || authority.contains('/') {
        return None;
    }
    let (_, host) = authority.rsplit_once('@')?;
    (!host.is_empty()).then_some(host)
}

/// The first override URL leaf whose origin is off the fetch allowlist, if any.
fn first_off_allowlist_override(
    value: &serde_json::Value,
    context: &RouteContext,
) -> Option<String> {
    match value {
        serde_json::Value::String(spec) => {
            fetch_is_off_allowlist(spec, context).then(|| spec.clone())
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| first_off_allowlist_override(item, context))
        }
        serde_json::Value::Object(map) => {
            map.values().find_map(|item| first_off_allowlist_override(item, context))
        }
        _ => None,
    }
}

fn forbidden_off_allowlist(target: &str) -> Response {
    json_error(
        StatusCode::FORBIDDEN,
        &format!(
            "{target:?} is not allowed by this pnpr server; the operator must declare its \
             registry as a public route or an upstream",
        ),
    )
}

/// Reject a `registries` map the client could not have loaded itself.
///
/// The same validation the config reader runs on the setting, so a request
/// cannot route one scope to two registries, or reach the resolver with a
/// declaration pnpm would have refused on disk. The message is the reader's,
/// with its registry URLs already redacted.
pub(super) fn reject_invalid_registries(request: &ResolveRequest) -> Option<Response> {
    pnpm_config::registries::validate_declarations(request.registries.iter())
        .err()
        .map(|error| json_error(StatusCode::BAD_REQUEST, &error.to_string()))
}

pub(super) fn reject_invalid_patch_hashes(request: &ResolveRequest) -> Option<Response> {
    let (selector, _) = request.patched_dependencies.as_ref()?.iter().find(|(_, hash)| {
        hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })?;
    Some(json_error(
        StatusCode::BAD_REQUEST,
        &format!("patchedDependencies entry {selector:?} does not contain a SHA-256 hex digest"),
    ))
}

/// Reject a request whose client-supplied URLs carry inline
/// `user:pass@host` credentials, before any fetch or cache write. Covers
/// the default and named registries, every dependency spec, catalog and
/// package-extension value, override values, and the tarball URLs of an input
/// lockfile — every surface a tarball/registry URL can reach the resolver (or
/// be echoed back) through.
/// Returns a `400` response when one is found.
pub(super) fn reject_inline_url_auth(request: &ResolveRequest) -> Option<Response> {
    let mut specs: Vec<&str> = Vec::new();
    if let Some(registry) = request.registry.as_deref() {
        specs.push(registry);
    }
    specs.extend(request.registries.keys().map(String::as_str));
    let projects = request.projects_normalized();
    for project in &projects {
        for map in
            [&project.dependencies, &project.dev_dependencies, &project.optional_dependencies]
        {
            specs.extend(map.values().map(String::as_str));
        }
    }
    if let Some(catalogs) = request.catalogs.as_ref() {
        specs.extend(catalogs.values().flat_map(|catalog| catalog.values()).map(String::as_str));
    }
    extend_package_extension_specs(request, &mut specs);
    // A supplied lockfile can carry `resolution.tarball` URLs that reach the
    // verify/frozen paths and would otherwise be routed or echoed back.
    if let Some(packages) =
        request.lockfile.as_ref().and_then(|lockfile| lockfile.packages.as_ref())
    {
        for package in packages.values() {
            if let LockfileResolution::Tarball(resolution) = &package.resolution {
                specs.push(resolution.tarball.as_str());
            }
        }
    }
    let inline = specs.iter().any(|spec| pnpr_route::url_has_inline_credentials(spec))
        || request.overrides.as_ref().is_some_and(overrides_have_inline_url_auth);
    inline.then(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "inline URL credentials (user:pass@host) are not allowed; \
             configure an upstream credential alias instead",
        )
    })
}

fn extend_package_extension_specs<'a>(request: &'a ResolveRequest, specs: &mut Vec<&'a str>) {
    let Some(extensions) = request.package_extensions.as_ref() else {
        return;
    };
    for extension in extensions.values() {
        for dependencies in [
            extension.dependencies.as_ref(),
            extension.optional_dependencies.as_ref(),
            extension.peer_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            specs.extend(dependencies.values().map(String::as_str));
        }
    }
}

/// Recursively scan an `overrides` JSON value for any string leaf that is
/// a URL carrying inline credentials.
fn overrides_have_inline_url_auth(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(spec) => pnpr_route::url_has_inline_credentials(spec),
        serde_json::Value::Array(items) => items.iter().any(overrides_have_inline_url_auth),
        serde_json::Value::Object(map) => map.values().any(overrides_have_inline_url_auth),
        _ => false,
    }
}
