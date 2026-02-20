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
}): string {
  if (opts.nonSemverVersion) {
    // Git-hosted or tarball dep — encode the raw version as a qualifier
    const encodedUrl = encodeURIComponent(opts.nonSemverVersion)
    return `pkg:npm/${encodePurlName(opts.name)}@${encodeURIComponent(opts.version)}?vcs_url=${encodedUrl}`
  }
  return `pkg:npm/${encodePurlName(opts.name)}@${opts.version}`
}
