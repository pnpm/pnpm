/**
 * Build the canonical tarball URL of an npm package — i.e. the URL pnpm derives
 * from a package's name, version, and registry. Vendored from the
 * `get-npm-tarball-url` package so the logic and its inverse
 * ({@link isCanonicalRegistryTarballUrl}) live together in the monorepo.
 */
export function getNpmTarballUrl (
  pkgName: string,
  pkgVersion: string,
  opts?: {
    registry?: string
  }
): string {
  const registry = normalizeRegistry(opts?.registry)
  const scopelessName = getScopelessName(pkgName)
  return `${registry}${pkgName}/-/${scopelessName}-${removeBuildMetadataFromVersion(pkgVersion)}.tgz`
}

/**
 * Whether `tarball` is the canonical npm registry URL or equivalent scoped
 * Artifactory URL derived from the package name, version, and registry. Either
 * URL can be dropped from the lockfile and rebuilt by {@link getNpmTarballUrl}.
 *
 * The lockfile writer uses this to decide whether to persist a tarball URL.
 * It is exported so custom resolvers (pnpmfile `resolvers`) can emit a URL the
 * writer will treat as canonical, instead of re-deriving pnpm's URL shape by
 * hand. A resolver fronting a proxy that serves tarballs on a non-canonical
 * path (e.g. an ephemeral `localhost:<port>`) can rewrite the resolved tarball
 * to `getNpmTarballUrl(name, version, { registry })` so nothing host-specific
 * is persisted to `pnpm-lock.yaml`.
 *
 * Percent-encoding is case-insensitive, so the `%2f` unescape matches both
 * `%2f` and `%2F` in the URLs npm produces for scoped packages.
 */
export function isCanonicalRegistryTarballUrl (
  tarball: string,
  pkg: { name: string, version: string },
  registry: string
): boolean {
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const normalizedExpectedTarball = removeProtocol(expectedTarball)
  const normalizedActualTarball = removeProtocol(tarball.replace(/%2f/gi, '/'))
  if (normalizedExpectedTarball === normalizedActualTarball) {
    return true
  }

  const scopeSeparator = pkg.name.indexOf('/')
  if (pkg.name[0] !== '@' || scopeSeparator === -1) {
    return false
  }
  const filenameStart = normalizedExpectedTarball.lastIndexOf('/-/') + 3
  const scope = pkg.name.slice(0, scopeSeparator)
  return `${normalizedExpectedTarball.slice(0, filenameStart)}${scope}/${normalizedExpectedTarball.slice(filenameStart)}` === normalizedActualTarball
}

function normalizeRegistry (registry?: string): string {
  if (!registry) return 'https://registry.npmjs.org/'
  return registry.endsWith('/') ? registry : `${registry}/`
}

function removeBuildMetadataFromVersion (version: string): string {
  const plusPos = version.indexOf('+')
  if (plusPos === -1) return version
  return version.substring(0, plusPos)
}

function getScopelessName (name: string): string {
  if (name[0] !== '@') {
    return name
  }
  return name.split('/')[1]
}

// Strips only a leading http(s) scheme so URLs are compared protocol-insensitively
// without truncating on a later `://` in the path or query.
function removeProtocol (url: string): string {
  return url.replace(/^https?:\/\//i, '')
}
