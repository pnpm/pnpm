import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { ResolutionVerification } from '@pnpm/resolving.resolver-base'
import { createRegistryRevisionsUnsupportedError } from '@pnpm/resolving.tarball-url'
import type { RegistriesByScope } from '@pnpm/types'

import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import { fetchAbbreviatedMeta, type PublishedAtLookupContext } from './verifierMetaLookup.js'
import { TARBALL_URL_MISMATCH_VIOLATION_CODE } from './violationCodes.js'

export type ResolutionViolation = Extract<ResolutionVerification, { ok: false }>

/** A registry lockfile entry, with the registry its metadata is fetched from. */
export interface RegistryPin {
  registry: string
  name: string
  version: string
  integrity: string
  tarballUrl: string | undefined
}

export interface RegistryRouting {
  registriesByScope: RegistriesByScope
  namedRegistryPrefixes: readonly string[]
}

/**
 * Confirm the lockfile-pinned tarball URL is the artifact the registry's
 * own metadata lists for this exact `name@version`.
 *
 * Fail-closed: the entry passes only when the registry metadata
 * affirmatively lists this version with a matching tarball URL. If the
 * metadata can't be fetched, doesn't list the version, or omits
 * `dist.tarball`, the entry can't be confirmed and is rejected — otherwise
 * a tampered lockfile could smuggle a malicious URL past the check by
 * pointing it at a `name@version` the registry can't vouch for.
 */
export async function runTarballUrlCheck (
  context: PublishedAtLookupContext,
  pin: RegistryPin & { tarballUrl: string }
): Promise<ResolutionViolation | undefined> {
  const { meta, error } = await fetchAbbreviatedMeta(context, pin.registry, pin.name)
  if (error != null) {
    // Couldn't reach the registry to verify (auth/network/5xx). Propagate the
    // registry's own fetch error (e.g. ERR_PNPM_FETCH_403, which already
    // explains the auth situation) instead of mislabeling a transport failure
    // as a tampering-style URL mismatch. The gate aborts the install with that
    // error — still fail-closed, the entry never reaches the filesystem.
    throw error
  }
  if (meta?.revisionVersions?.has(pin.version)) {
    throw createRegistryRevisionsUnsupportedError(
      `The registry serves ${pin.name}@${pin.version} as a registry revision`
    )
  }
  const registryTarball = meta?.versionTarballs?.get(pin.version)
  if (typeof registryTarball === 'string' && sameTarballUrl(pin.tarballUrl, registryTarball)) return undefined
  return {
    ok: false,
    code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
    reason: typeof registryTarball !== 'string'
      ? "could not be verified against the registry's published metadata"
      : `has a tarball URL (${pin.tarballUrl}) that does not match the registry's published metadata (${registryTarball})`,
  }
}

function sameTarballUrl (leftUrl: string, rightUrl: string): boolean {
  return canonicalTarballUrl(leftUrl) === canonicalTarballUrl(rightUrl)
}

// Both URLs come from the registry, so ignore the protocol and `%2f` scope
// encoding: a benign http/https or encoding difference isn't tampering. The
// `%2f` match is case-insensitive because `normalizeRegistryUrl`
// (`new URL().toString()`) can upper-case percent-escapes to `%2F`.
function canonicalTarballUrl (url: string): string {
  const normalized = normalizeRegistryUrl(url).replace(/%2f/gi, '/')
  const schemeEnd = normalized.indexOf('://')
  return schemeEnd === -1 ? normalized : normalized.slice(schemeEnd + 3)
}

export function pickRegistryForVersion (
  routing: RegistryRouting,
  name: string,
  tarballUrl: string | undefined
): string {
  // If the lockfile records where the tarball lives, prefer that — scope
  // routing (`@scope:registry`) only covers scoped packages, but named
  // registries (`gh:`, `jsr:` aliases, custom) ship un-scoped packages whose
  // origin we'd otherwise miss. Match the longest prefix so that two named
  // registries sharing a host but differing by path don't collide.
  if (tarballUrl) {
    const tarballRegistry = findRegistryOfTarball(routing, name, tarballUrl)
    if (tarballRegistry != null) return tarballRegistry
  }
  return pickRegistryForPackage(routing.registriesByScope, name)
}

// Match on the same canonical form the tarball comparison uses, so a
// named-registry tarball that differs from the configured base only by
// scheme or `%2f` encoding still routes to its registry instead of
// falling back (and then failing closed against the wrong packument).
function findRegistryOfTarball (
  routing: RegistryRouting,
  name: string,
  tarballUrl: string
): string | undefined {
  const normalized = canonicalTarballUrl(tarballUrl)
  const candidatePrefixes = new Set(listCandidateRegistryPrefixes(routing, name))
  return [...candidatePrefixes].find(prefix => normalized.startsWith(canonicalTarballUrl(prefix)))
}

// A package whose scope has a registry of its own only matches that
// registry among the scope registries: the lockfile must not move
// `@a/pkg` off the registry `@a` is assigned to, or a registry that also
// proxies the public one would vouch for a same-name public package.
function listCandidateRegistryPrefixes (routing: RegistryRouting, name: string): string[] {
  const { registriesByScope } = routing
  const scope = name.startsWith('@') ? name.slice(0, name.indexOf('/')) : undefined
  const ownScopeRegistry = scope == null ? undefined : registriesByScope[scope]
  const scopeRegistries = ownScopeRegistry != null
    ? [ownScopeRegistry]
    : Object.entries(registriesByScope)
      .filter((entry): entry is [string, string] => entry[0] !== 'default' && typeof entry[1] === 'string')
      .map(([, url]) => url)
  return [...routing.namedRegistryPrefixes, ...scopeRegistries].sort(compareByCanonicalLengthDescending)
}

function compareByCanonicalLengthDescending (leftPrefix: string, rightPrefix: string): number {
  const diff = canonicalTarballUrl(rightPrefix).length - canonicalTarballUrl(leftPrefix).length
  return diff !== 0 ? diff : leftPrefix.localeCompare(rightPrefix)
}
