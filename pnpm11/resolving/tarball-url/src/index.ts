import { Buffer } from 'node:buffer'

import type { RegistryServerType } from '@pnpm/types'

const PUBLIC_NPM_REGISTRY = 'https://registry.npmjs.org/'

/**
 * registry.npmjs.org is the one registry whose layout pnpm knows without being
 * told, so it is a row of data rather than a hostname comparison. A declared
 * `serverType` wins over it.
 */
const DEFAULT_REGISTRY_SERVER_TYPES: Record<string, RegistryServerType> = {
  [PUBLIC_NPM_REGISTRY]: 'npm',
}

export interface TarballUrlOptions {
  registry?: string
  /**
   * Undeclared by default, which is the strict reading: only the exact
   * canonical URL is reconstructible. See {@link RegistryServerType}.
   */
  serverType?: RegistryServerType
}

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

/**
 * Build the canonical tarball URL of an npm package — i.e. the URL pnpm derives
 * from a package's name, version, and registry. Vendored from the
 * `get-npm-tarball-url` package so the logic and its inverse
 * ({@link isCanonicalRegistryTarballUrl}) live together in the monorepo.
 *
 * This is the single source of the URL shape: the lockfile writer drops a
 * tarball URL only when this function rebuilds it, and the lockfile reader
 * rebuilds it with this function. Both sides therefore agree by construction,
 * under every {@link RegistryServerType}.
 */
export function getNpmTarballUrl (
  pkgName: string,
  pkgVersion: string,
  opts?: TarballUrlOptions
): string {
  const registry = normalizeRegistry(opts?.registry)
  // Artifactory keeps the scope in the filename of a scoped package's tarball
  // (`@acme/widget/-/@acme/widget-1.0.0.tgz`); the npm layout strips it.
  const filenameName = opts?.serverType === 'artifactory' ? pkgName : getScopelessName(pkgName)
  return `${registry}${pkgName}/-/${filenameName}-${removeBuildMetadataFromVersion(pkgVersion)}.tgz`
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
 * A `serverType` the user declared (via `getRegistryServerType` in
 * `@pnpm/config.normalize-registries`) wins; otherwise the built-in layout of
 * a known registry applies, and an unknown registry is read strictly.
 */
export function isCanonicalRegistryTarballUrl (
  tarball: string,
  pkg: { name: string, version: string },
  opts: TarballUrlOptions
): boolean {
  const expectedTarball = removeProtocol(getNpmTarballUrl(pkg.name, pkg.version, opts))
  const actualTarball = removeProtocol(tarball)
  if (expectedTarball === actualTarball) return true
  // A registry behaving like registry.npmjs.org serves a scoped package from
  // both the encoded and the unencoded path. A registry that has not been
  // declared to behave like it may serve only the encoded one, so its URL is
  // kept. See https://github.com/pnpm/pnpm/issues/13534.
  return effectiveServerType(opts) === 'npm' && expectedTarball === actualTarball.replace(/%2f/gi, '/')
}

function effectiveServerType (opts: TarballUrlOptions): RegistryServerType | undefined {
  return opts.serverType ?? DEFAULT_REGISTRY_SERVER_TYPES[normalizeRegistry(opts.registry)]
}

function normalizeRegistry (registry?: string): string {
  if (!registry) return PUBLIC_NPM_REGISTRY
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
