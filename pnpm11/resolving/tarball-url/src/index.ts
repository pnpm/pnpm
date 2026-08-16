const PUBLIC_NPM_REGISTRY = 'https://registry.npmjs.org/'

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
 * Whether `tarball` is the canonical npm registry URL derived from the package
 * name, version, and registry — i.e. it can be dropped from the lockfile and
 * rebuilt on demand by {@link getNpmTarballUrl}.
 *
 * The lockfile writer uses this to decide whether to persist a tarball URL.
 * It is exported so custom resolvers (pnpmfile `resolvers`) can emit a URL the
 * writer will treat as canonical, instead of re-deriving pnpm's URL shape by
 * hand. A resolver fronting a proxy that serves tarballs on a non-canonical
 * path (e.g. an ephemeral `localhost:<port>`) can rewrite the resolved tarball
 * to `getNpmTarballUrl(name, version, { registry })` so nothing host-specific
 * is persisted to `pnpm-lock.yaml`.
 */
export function isCanonicalRegistryTarballUrl (
  tarball: string,
  pkg: { name: string, version: string },
  registry: string
): boolean {
  const expectedTarball = removeProtocol(getNpmTarballUrl(pkg.name, pkg.version, { registry }))
  const actualTarball = removeProtocol(tarball)
  if (expectedTarball === actualTarball) return true
  // registry.npmjs.org serves a scoped package from both the encoded and the
  // unencoded path; elsewhere only the encoded one may work.
  // See https://github.com/pnpm/pnpm/issues/13534.
  return isPublicNpmRegistry(registry) && expectedTarball === actualTarball.replace(/%2f/gi, '/')
}

function isPublicNpmRegistry (registry: string): boolean {
  return removeProtocol(normalizeRegistry(registry)) === removeProtocol(PUBLIC_NPM_REGISTRY)
}

function normalizeRegistry (registry?: string): string {
  if (!registry) return PUBLIC_NPM_REGISTRY
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
