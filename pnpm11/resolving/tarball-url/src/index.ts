import { Buffer } from 'node:buffer'

export interface RevisionAwareIntegrity {
  algorithm: 'sha512'
  contentIntegrity: string
  digest: Buffer
  revision: string
}

export function parseRevisionAwareIntegrity (integrity: string): RevisionAwareIntegrity | undefined {
  const optionSeparator = integrity.indexOf('?')
  if (optionSeparator === -1 || optionSeparator !== integrity.lastIndexOf('?')) return undefined

  const option = integrity.slice(optionSeparator + 1)
  if (!isCanonicalRevisionOption(option)) return undefined

  const contentIntegrity = integrity.slice(0, optionSeparator)
  const algorithmSeparator = contentIntegrity.indexOf('-')
  if (algorithmSeparator === -1 || contentIntegrity.indexOf('-', algorithmSeparator + 1) !== -1) {
    return undefined
  }
  const algorithm = contentIntegrity.slice(0, algorithmSeparator)
  if (algorithm !== 'sha512') return undefined

  const encodedDigest = contentIntegrity.slice(algorithmSeparator + 1)
  const digest = Buffer.from(encodedDigest, 'base64')
  if (digest.byteLength !== 64 || digest.toString('base64') !== encodedDigest) return undefined

  return {
    algorithm,
    contentIntegrity,
    digest,
    revision: option.slice(1),
  }
}

export function getIntegrityAddressedTarballUrl (
  integrity: string,
  registry: string
): string | undefined {
  const parsed = parseRevisionAwareIntegrity(integrity)
  if (parsed == null) return undefined
  return new URL(
    `-/tarballs/${parsed.algorithm}/${parsed.digest.toString('base64url')}`,
    normalizeRegistry(registry)
  ).toString()
}

export function isIntegrityAddressedRegistryTarballUrl (
  tarball: string,
  integrity: string,
  registry: string
): boolean {
  const expected = getIntegrityAddressedTarballUrl(integrity, registry)
  if (expected == null) return false
  try {
    return new URL(tarball).toString() === expected
  } catch {
    return false
  }
}

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
  const actualTarball = tarball.replace(/%2f/gi, '/')
  return removeProtocol(expectedTarball) === removeProtocol(actualTarball)
}

function normalizeRegistry (registry?: string): string {
  if (!registry) return 'https://registry.npmjs.org/'
  return registry.endsWith('/') ? registry : `${registry}/`
}

function isCanonicalRevisionOption (option: string): boolean {
  if (option === 'r0') return true
  if (option.length < 2 || option[0] !== 'r' || option[1] === '0') return false
  for (let index = 1; index < option.length; index++) {
    const code = option.charCodeAt(index)
    if (code < 48 || code > 57) return false
  }
  return true
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
