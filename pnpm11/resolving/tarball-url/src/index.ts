import { Buffer } from 'node:buffer'
import util from 'node:util'

import { formatIntegrity, parseIntegrity } from '@pnpm/crypto.integrity'
import type { RegistryServerType } from '@pnpm/types'

const PUBLIC_NPM_REGISTRY = 'https://registry.npmjs.org/'
const SHA512_INTEGRITY_LENGTH = 'sha512-'.length + 88

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

export interface IntegrityAddress {
  algorithm: 'sha512'
  digest: Buffer
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

export function getIntegrityAddressedTarballUrl (
  integrity: unknown,
  registry: string
): string | undefined {
  const parsed = parseIntegrityAddress(integrity)
  if (parsed == null) return undefined
  return new URL(
    `-/tarballs/${parsed.algorithm}/${parsed.digest.toString('base64url')}`,
    normalizeRegistry(registry)
  ).toString()
}

export function parseIntegrityAddress (integrity: unknown): IntegrityAddress | undefined {
  if (typeof integrity !== 'string' || integrity.length !== SHA512_INTEGRITY_LENGTH) return undefined
  let parsed: ReturnType<typeof parseIntegrity>
  try {
    parsed = parseIntegrity(integrity)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_INVALID_INTEGRITY') {
      return undefined
    }
    throw err
  }
  if (
    parsed.algorithm !== 'sha512' ||
    parsed.hexDigest.length !== 128 ||
    formatIntegrity(parsed.algorithm, parsed.hexDigest) !== integrity
  ) return undefined
  return {
    algorithm: parsed.algorithm,
    digest: Buffer.from(parsed.hexDigest, 'hex'),
  }
}

export function isValidTarballRevision (revision: unknown): revision is number {
  return typeof revision === 'number' &&
    Number.isSafeInteger(revision) &&
    revision > 0
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
