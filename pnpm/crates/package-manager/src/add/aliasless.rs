use super::{AddError, AddResolveInputs, normalized_save_specifier};
use pnpm_config::Config;
use pnpm_network::{ThrottledClient, redact_and_sanitize, redact_url_for_display};
use pnpm_package_manifest::PackageManifest;
use pnpm_package_name::is_valid_dependency_alias;
use pnpm_resolving_git_resolver::{
    GitFetchContext, GitResolver, HostedGit, RealGitProbe, RealGitRunner,
};
use pnpm_resolving_local_resolver::{
    LocalResolverContext, LocalResolverOptions, LocalResolverUpdate, WantedLocalDependency,
    is_local_filesystem_specifier, resolve_from_local_path, resolve_from_local_scheme,
};
use pnpm_resolving_resolver_base::GitResolveError;
use pnpm_resolving_tarball_resolver::{TarballFetchContext, TarballResolver};
use pnpm_store_dir::SharedVerifiedFilesCache;
use std::{collections::HashMap, sync::Arc};

/// The package name and manifest specifier for an add selector that names
/// no alias: a git URL, a remote tarball URL, or a local directory /
/// tarball. Such a selector is the whole specifier, and the package's name
/// lives only in its own manifest — inside the archive, the checkout, or
/// the directory — so it has to be read from there before the manifest
/// entry can be written.
pub(super) struct AliaslessDependency {
    pub(super) package_name: String,
    pub(super) manifest_specifier: String,
}
/// Resolve an alias-less add selector, or `None` to leave it to
/// [`split_name_spec`](crate::add::specifier::split_name_spec), which reads it as a registry `<name>[@<spec>]`.
///
/// Dispatch order mirrors the install's resolver chain: git and the tarball
/// URL are matched first, because the local-path detector would otherwise
/// claim those shapes on the strength of an embedded `/` or `:`.
pub(super) async fn resolve_aliasless_specifier(
    specifier: &str,
    inputs: &AddResolveInputs<'_, '_>,
    manifest: &PackageManifest,
) -> Result<Option<AliaslessDependency>, AddError> {
    if pnpm_resolving_git_resolver::parse_bare_specifier(specifier).is_some() {
        return resolve_aliasless_git(specifier, inputs).await.map(Some);
    }
    if specifier.starts_with("http:") || specifier.starts_with("https:") {
        return resolve_aliasless_tarball(specifier, inputs.add.config, inputs.http_client_arc)
            .await
            .map(Some);
    }
    resolve_aliasless_local(specifier, manifest).await
}
/// Resolve a local directory or tarball selector by reading the package's
/// own manifest — off disk for a directory, out of the archive for a
/// tarball.
///
/// `link:` / `file:` are claimed by their scheme, everything else by path
/// shape ([`is_local_filesystem_specifier`]).
pub(super) async fn resolve_aliasless_local(
    specifier: &str,
    manifest: &PackageManifest,
) -> Result<Option<AliaslessDependency>, AddError> {
    if !is_local_filesystem_specifier(specifier) {
        return Ok(None);
    }
    let wanted = WantedLocalDependency { bare_specifier: specifier.to_string(), injected: false };
    let ctx = LocalResolverContext { preserve_absolute_paths: false };
    let opts = LocalResolverOptions {
        project_dir: manifest
            .path()
            .parent()
            .expect("manifest path always has a parent dir")
            .to_path_buf(),
        // The name and the normalized specifier are all this resolve is
        // after, and neither is measured from the lockfile root.
        lockfile_dir: None,
        current_pkg: None,
        update: LocalResolverUpdate::On,
    };
    let mut claimed =
        resolve_from_local_scheme(&ctx, &wanted, &opts).await.map_err(AddError::ResolveLocal)?;
    if claimed.is_none() {
        claimed =
            resolve_from_local_path(&ctx, &wanted, &opts).await.map_err(AddError::ResolveLocal)?;
    }
    let Some(resolved) = claimed else {
        return Ok(None);
    };
    let manifest_specifier =
        resolved.normalized_bare_specifier.unwrap_or_else(|| normalized_save_specifier(specifier));
    let package_name = aliasless_package_name(resolved.manifest.as_deref(), specifier)?;
    Ok(Some(AliaslessDependency { package_name, manifest_specifier }))
}
/// Resolve a remote (non-registry) tarball URL by downloading and
/// extracting it: a tarball's name lives in the `package.json` it bundles,
/// and nothing in the URL is required to spell it.
///
/// No store-index row is written here. The install that follows re-resolves
/// the dependency with the writer it owns, keyed by the same `pkg_id`, so a
/// row from this pass would only duplicate that one.
pub(super) async fn resolve_aliasless_tarball(
    specifier: &str,
    config: &'static Config,
    http_client: &Arc<ThrottledClient>,
) -> Result<AliaslessDependency, AddError> {
    let resolver = TarballResolver {
        http_client: Arc::clone(http_client),
        fetch_context: Some(TarballFetchContext {
            store_dir: &config.store_dir,
            store_index_writer: None,
            mem_cache: None,
            auth_headers: Arc::clone(&config.auth_headers),
            retry_opts: crate::retry_config::retry_opts_from_config(config),
            store_index: None,
            verify_store_integrity: config.verify_store_integrity,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            prior_tarball_entries: Arc::new(HashMap::new()),
        }),
    };
    let wanted = pnpm_resolving_resolver_base::WantedDependency {
        bare_specifier: Some(specifier.to_string()),
        ..pnpm_resolving_resolver_base::WantedDependency::default()
    };
    let result = pnpm_resolving_resolver_base::Resolver::resolve(
        &resolver,
        &wanted,
        &pnpm_resolving_resolver_base::ResolveOptions::default(),
    )
    .await
    .map_err(|source| match source.downcast::<pnpm_tarball::TarballError>() {
        Ok(tarball) => AddError::TarballResolve(tarball),
        Err(source) => AddError::ResolveTarball {
            specifier: redact_url_for_display(specifier),
            reason: redacted_error_chain(source.as_ref()),
        },
    })?
    .ok_or_else(|| AddError::MissingPackageName { specifier: redact_url_for_display(specifier) })?;
    let manifest_specifier =
        result.normalized_bare_specifier.unwrap_or_else(|| normalized_save_specifier(specifier));
    let package_name = aliasless_package_name(result.manifest.as_deref(), specifier)?;
    Ok(AliaslessDependency { package_name, manifest_specifier })
}
/// Flatten an error chain into one line, with every URL in it cut back to
/// its display-safe form.
///
/// `reqwest` echoes the request URL back in its own message and redacts
/// only the userinfo, while a signed tarball URL carries its token in the
/// query string — which [`redact_and_sanitize`] keeps too. The scrub is
/// URL-agnostic rather than keyed to the specifier: the HEAD request
/// follows redirects, so the URL a failure names may be a signed storage
/// URL the caller never saw.
pub(super) fn redacted_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut frames = Vec::new();
    let mut current = Some(error);
    while let Some(frame) = current {
        frames.push(frame.to_string());
        current = frame.source();
    }
    // Order is load-bearing: `redact_and_sanitize` strips the `user:pass@`
    // and the control characters, leaving each URL as a plain token for the
    // query/fragment cut below.
    strip_url_query_and_fragment(&redact_and_sanitize(&frames.join(": ")))
}
/// Cut every URL in `text` back to its display-safe form. A URL in error
/// prose ends at whitespace or at one of the marks a message wraps it in,
/// and a `://` not preceded by a scheme is not an authority boundary.
pub(super) fn strip_url_query_and_fragment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        // Walk back over the scheme so the whole URL token can be replaced,
        // not just its authority onwards.
        let scheme_start = rest[..pos]
            .rfind(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.'))
            })
            .map_or(0, |index| index + 1);
        if scheme_start == pos {
            out.push_str(&rest[..pos + "://".len()]);
            rest = &rest[pos + "://".len()..];
            continue;
        }
        out.push_str(&rest[..scheme_start]);
        let token_and_tail = &rest[scheme_start..];
        let end = url_token_len(token_and_tail);
        let (token, tail) = token_and_tail.split_at(end);
        out.push_str(&redact_url_token(token));
        rest = tail;
    }
    out.push_str(rest);
    out
}
/// Where the URL at the start of `text` ends: at whitespace, at one of the
/// marks a message wraps a URL in, or at a `": "` — which cannot occur
/// inside a URL, and is how a message punctuates the URL from what follows
/// it (`GET <url>: 404`). Without that last case, cutting the query would
/// swallow the message's own separator.
pub(super) fn url_token_len(text: &str) -> usize {
    let wrapped = text
        .find(|character: char| {
            character.is_whitespace() || matches!(character, ')' | ']' | '"' | '\'')
        })
        .unwrap_or(text.len());
    text.find(": ").map_or(wrapped, |punctuated| wrapped.min(punctuated))
}
/// One URL token cut at its query or fragment, or `[hidden]` when the cut
/// would leave credential material behind.
///
/// A password containing `?` or `#` defeats the userinfo scan that runs
/// before this — the scan reads the `?` as the end of the authority and
/// leaves the whole `user:pa?ss@host` in place — so cutting there would
/// publish the password's prefix. The authority is therefore checked on the
/// *uncut* token: any `@` still in front of the path means the userinfo
/// survived, and the token fails closed instead.
pub(super) fn redact_url_token(token: &str) -> String {
    let after_scheme = token.find("://").map_or(token, |pos| &token[pos + "://".len()..]);
    let authority = &after_scheme[..after_scheme.find('/').unwrap_or(after_scheme.len())];
    if authority.contains('@') {
        return "[hidden]".to_string();
    }
    token[..token.find(['?', '#']).unwrap_or(token.len())].to_string()
}
/// The display-safe form of an alias-less selector, which may be a local
/// path or a URL. A URL loses its credentials, query, and fragment; a path
/// is only stripped of control characters, since it carries no secret and
/// naming it is the whole point of the message.
pub(super) fn redact_specifier_for_display(specifier: &str) -> String {
    if specifier.starts_with("http:") || specifier.starts_with("https:") {
        redact_url_for_display(specifier)
    } else {
        redact_and_sanitize(specifier)
    }
}
/// The `name` an alias-less selector's own manifest declares. The name keys
/// the project's manifest entry and names the `node_modules` directory the
/// package is linked into, so a missing or invalid one is refused rather
/// than guessed at.
pub(super) fn aliasless_package_name(
    manifest: Option<&serde_json::Value>,
    specifier: &str,
) -> Result<String, AddError> {
    let name = manifest
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AddError::MissingPackageName {
            specifier: redact_specifier_for_display(specifier),
        })?;
    if !is_valid_dependency_alias(name) {
        return Err(AddError::InvalidPackageName {
            specifier: redact_specifier_for_display(specifier),
            name: name.to_string(),
        });
    }
    Ok(name.to_string())
}
pub(super) async fn resolve_aliasless_git(
    specifier: &str,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<AliaslessDependency, AddError> {
    let resolver = aliasless_git_resolver(inputs);
    let wanted = pnpm_resolving_resolver_base::WantedDependency {
        bare_specifier: Some(specifier.to_string()),
        ..pnpm_resolving_resolver_base::WantedDependency::default()
    };
    let result = pnpm_resolving_resolver_base::Resolver::resolve(
        &resolver,
        &wanted,
        &pnpm_resolving_resolver_base::ResolveOptions::default(),
    )
    .await
    .map_err(|source| match source.downcast::<GitResolveError>() {
        Ok(git_resolve) => AddError::GitResolve(*git_resolve),
        // A specifier can carry `user:pass@` credentials, and every error here
        // echoes it back.
        Err(source) => AddError::ResolveGit { specifier: redact_and_sanitize(specifier), source },
    })?
    .ok_or_else(|| AddError::GitPackageName { specifier: redact_and_sanitize(specifier) })?;
    let package_name = result
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| HostedGit::from_url(specifier).map(|hosted| hosted.project))
        .ok_or_else(|| AddError::GitPackageName { specifier: redact_and_sanitize(specifier) })?;
    if !is_valid_dependency_alias(&package_name) {
        return Err(AddError::InvalidGitPackageName {
            specifier: redact_and_sanitize(specifier),
            name: package_name,
        });
    }
    let manifest_specifier =
        result.normalized_bare_specifier.unwrap_or_else(|| normalized_save_specifier(specifier));
    Ok(AliaslessDependency { package_name, manifest_specifier })
}
pub(super) fn aliasless_git_resolver(
    inputs: &AddResolveInputs<'_, '_>,
) -> GitResolver<RealGitProbe, RealGitRunner> {
    let config = inputs.add.config;
    let http_client = inputs.http_client_arc;
    GitResolver::new(
        Arc::new(RealGitProbe::new(Arc::clone(http_client))),
        Arc::new(RealGitRunner::new()),
    )
    .with_fetch_context(GitFetchContext {
        source_cache: Arc::clone(inputs.git_source_cache),
        http_client: Arc::clone(http_client),
        store_dir: &config.store_dir,
        store_index_writer: None,
        auth_headers: Arc::clone(&config.auth_headers),
        retry_opts: crate::retry_config::retry_opts_from_config(config),
        git_shallow_hosts: config.git_shallow_hosts.clone(),
    })
}
