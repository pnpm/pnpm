use super::{
    CanonicalPackageName,
    TarballRouter,
    revision_route_tarball_version,
    tarball_basename,
    tarball_basename_for_package,
    version_route_tarball_version,
};

impl TarballRouter {
    /// Reverse a named endpoint tarball URL back to its upstream URL so an
    /// input lockfile carrying endpoint URLs can be verified against the real
    /// registry. Returns `None` for any other URL, and for an endpoint the
    /// caller is not authorized for (so verification cannot be used as an
    /// oracle for an upstream the caller cannot reach).
    pub(in crate::resolver::wire) fn upstream_endpoint_tarball_url(
        &self,
        tarball_url: &str,
        package: Option<&str>,
    ) -> Option<String> {
        let prefix = format!("{}/~", npm_endpoint(&self.public_url));
        let route = tarball_url.strip_prefix(&prefix)?;
        let (upstream, rest) = route.split_once('/')?;
        let registry = self.context.upstream_registry(&self.identity, upstream, package)?;
        Some(format!("{}/{rest}", registry.trim_end_matches('/')))
    }
}

pub(super) fn tarball_route_tail(package: &str, version: &str, tarball_url: &str) -> String {
    if let Ok(name) = CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm)
        && let Some(route_version) = version_route_tarball_version(tarball_url, &name)
            .or_else(|| revision_route_tarball_version(tarball_url, &name))
    {
        return format!("{package}@{route_version}");
    }
    let filename = match CanonicalPackageName::parse(package, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => tarball_basename_for_package(tarball_url, &name)
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_else(|| name.tarball_name_for_version(version)),
        Err(_) => tarball_basename(tarball_url)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{package}-{version}.tgz")),
    };
    format!("{package}/-/{filename}")
}

pub(super) fn pnpr_tarball_url(
    public_url: &str,
    source_url: &str,
    package: &str,
    version: &str,
    tarball_url: &str,
) -> String {
    format!(
        "{}/{}",
        hosted_endpoint(public_url, source_url),
        tarball_route_tail(package, version, tarball_url),
    )
}

fn hosted_endpoint(public_url: &str, source_url: &str) -> String {
    let base = npm_endpoint(public_url);
    let addressed_prefix = format!("{base}/~");
    if let Some(rest) = source_url.strip_prefix(&addressed_prefix) {
        let name = rest.split('/').next().unwrap_or(rest);
        if !name.is_empty() {
            return format!("{addressed_prefix}{name}");
        }
    }
    base
}

/// The registry-endpoint URL a proxied route's tarball is served through.
pub(super) fn upstream_endpoint_tarball_url(
    public_url: &str,
    upstream: &str,
    package: &str,
    version: &str,
    tarball_url: &str,
) -> String {
    format!(
        "{}/~{upstream}/{}",
        npm_endpoint(public_url),
        tarball_route_tail(package, version, tarball_url),
    )
}

fn npm_endpoint(public_url: &str) -> String {
    format!("{}{}", public_url.trim_end_matches('/'), pnpr_registry::Ecosystem::Npm.base_path())
}
