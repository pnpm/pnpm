/**
 * Encode a package name for use in a PURL.
 * Scoped packages: @scope/name → %40scope/name
 */
export function encodePurlName (name: string): string {
  if (name.startsWith('@')) {
    return `%40${name.slice(1)}`
  }
  return name
}

/**
 * Build a Package URL (PURL) for a given package.
 * Spec: https://github.com/package-url/purl-spec
 */
export function buildPurl (opts: {
  name: string
  version: string
  nonSemverVersion?: string
  /**
   * Registry the package was resolved from, when it is not the default
   * one. Emitted as the spec's `repository_url` qualifier, which is what
   * keeps the same name and version served by two registries from
   * collapsing onto one component.
   */
  registryUrl?: string
}): string {
  if (opts.nonSemverVersion) {
    // Git-hosted or tarball dep — encode the raw version as a qualifier
    const encodedUrl = encodeURIComponent(opts.nonSemverVersion)
    return `pkg:npm/${encodePurlName(opts.name)}@${encodeURIComponent(opts.version)}?vcs_url=${encodedUrl}`
  }
  if (opts.registryUrl) {
    const repositoryUrl = sanitizeRegistryUrl(opts.registryUrl)
    return `pkg:npm/${encodePurlName(opts.name)}@${opts.version}?repository_url=${encodeURIComponent(repositoryUrl)}`
  }
  return `pkg:npm/${encodePurlName(opts.name)}@${opts.version}`
}

/**
 * Reduce a registry URL to the parts that identify the registry, dropping
 * anything that could carry a secret.
 *
 * An SBOM is meant to be published, and a `registriesByPrefix` entry may
 * legitimately embed credentials — as userinfo
 * (`https://user:token@npm.example.com/`) or in a query string
 * (`?api_key=…`). Origin and path are kept because two registries can
 * differ only by path (`https://npm.example.com/team-a/` vs `/team-b/`),
 * so trimming to the origin would recreate the very collision this
 * qualifier exists to prevent.
 *
 * A URL that doesn't parse is returned unchanged: it has no userinfo to
 * strip, and `registriesByPrefix` values are validated as http(s) URLs when
 * the resolver is constructed, so this is unreachable in practice.
 */
function sanitizeRegistryUrl (url: string): string {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return url
  }
  parsed.username = ''
  parsed.password = ''
  parsed.search = ''
  parsed.hash = ''
  return parsed.toString()
}
