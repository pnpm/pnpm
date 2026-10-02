import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { ResolutionVerification } from '@pnpm/resolving.resolver-base'
import {
  isIntegrityAddressedRegistryTarballUrl,
  isValidTarballRevision,
} from '@pnpm/resolving.tarball-url'
import type { RegistriesByScope } from '@pnpm/types'

import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import {
  fetchAbbreviatedMeta,
  type PublishedAtLookupContext,
  type RegistryArtifact,
  type RegistryArtifactHistory,
} from './verifierMetaLookup.js'
import {
  TARBALL_REVISION_MISMATCH_VIOLATION_CODE,
  TARBALL_URL_MISMATCH_VIOLATION_CODE,
} from './violationCodes.js'

export type ResolutionViolation = Extract<ResolutionVerification, { ok: false }>

/** A registry lockfile entry, with the registry its metadata is fetched from. */
export interface RegistryPin {
  registry: string
  name: string
  version: string
  integrity: string
  rawRevision: unknown
  tarballUrl: string | undefined
}

export interface RegistryRouting {
  registriesByScope: RegistriesByScope
  namedRegistryPrefixes: readonly string[]
}

interface RevisionSelection {
  revision: number
  currentMatches: boolean
  historical?: RegistryArtifact
  selected?: RegistryArtifact
}

const UNVERIFIABLE_REASON = "could not be verified against the registry's published metadata"

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
export async function runRegistryArtifactCheck (
  context: PublishedAtLookupContext,
  pin: RegistryPin
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
  const artifact = meta?.versionArtifacts?.get(pin.version)
  if (artifact == null) return reportUnlistedArtifact(pin)
  if (!isRevisionAware(artifact, pin.rawRevision)) {
    if (pin.tarballUrl == null) return undefined
    return checkTarballUrlMatches(pin.tarballUrl, artifact.current.tarball)
  }
  return checkRevisionedArtifact(artifact, pin)
}

function reportUnlistedArtifact (pin: RegistryPin): ResolutionViolation | undefined {
  if (pin.tarballUrl == null && pin.rawRevision == null) return undefined
  return {
    ok: false,
    code: pin.tarballUrl == null
      ? TARBALL_REVISION_MISMATCH_VIOLATION_CODE
      : TARBALL_URL_MISMATCH_VIOLATION_CODE,
    reason: UNVERIFIABLE_REASON,
  }
}

function isRevisionAware (artifact: RegistryArtifactHistory, rawRevision: unknown): boolean {
  return rawRevision != null || artifact.current.revision !== undefined || artifact.revisions.length > 0
}

function checkRevisionedArtifact (
  artifact: RegistryArtifactHistory,
  pin: RegistryPin
): ResolutionViolation | undefined {
  if (pin.rawRevision != null && !isValidTarballRevision(pin.rawRevision)) {
    return revisionMismatch(`has an invalid revision (${String(pin.rawRevision)})`)
  }
  const currentRevision = readCurrentRevision(artifact, pin.registry)
  if (typeof currentRevision !== 'number') return currentRevision
  const selection = selectPinnedRevision(artifact, typeof pin.rawRevision === 'number' ? pin.rawRevision : 0, currentRevision)
  if ('ok' in selection) return selection
  const selected = pickIntegrityMatchedArtifact(selection, pin.integrity)
  if (selected == null) {
    return revisionMismatch(`has revision ${selection.revision} with an integrity that does not match the registry's current or historical metadata`)
  }
  return checkSelectedArtifactLocation(selected, selection, pin)
}

function readCurrentRevision (
  artifact: RegistryArtifactHistory,
  registry: string
): number | ResolutionViolation {
  const metadataRevision = artifact.current.revision
  if (metadataRevision === undefined) return 0
  if (!isValidTarballRevision(metadataRevision)) {
    return revisionMismatch(`registry metadata has an invalid current revision (${String(metadataRevision)})`)
  }
  const currentHistory = artifact.revisions.filter(candidate => candidate.revision === metadataRevision)
  if (!isConsistentCurrentRevision(artifact.current, currentHistory, registry)) {
    return revisionMismatch(`registry metadata revision ${metadataRevision} does not have exactly one matching history entry`)
  }
  return metadataRevision
}

function selectPinnedRevision (
  artifact: RegistryArtifactHistory,
  revision: number,
  currentRevision: number
): RevisionSelection | ResolutionViolation {
  const currentMatches = currentRevision === revision
  const historicalCandidates = artifact.revisions.filter(candidate => candidate.revision === revision)
  if (historicalCandidates.length > 1) {
    return revisionMismatch(`revision ${revision} is advertised more than once in the registry's history`)
  }
  const historical = historicalCandidates[0]
  return {
    revision,
    currentMatches,
    historical,
    selected: currentMatches ? artifact.current : historical,
  }
}

function pickIntegrityMatchedArtifact (
  selection: RevisionSelection,
  lockfileIntegrity: string
): RegistryArtifact | undefined {
  const { selected, historical, currentMatches } = selection
  if (selected == null || selected.integrity !== lockfileIntegrity) return undefined
  if (currentMatches && historical != null && historical.integrity !== lockfileIntegrity) return undefined
  return selected
}

function checkSelectedArtifactLocation (
  selected: RegistryArtifact,
  selection: RevisionSelection,
  pin: RegistryPin
): ResolutionViolation | undefined {
  const mustBeIntegrityAddressed = selection.revision > 0 || !selection.currentMatches
  if (mustBeIntegrityAddressed && !isIntegrityAddressedTarball(selected.tarball, pin)) {
    return revisionMismatch(`has revision ${selection.revision} that is not addressed by its complete sha512 integrity`)
  }
  if (pin.tarballUrl == null) return undefined
  return checkTarballUrlMatches(pin.tarballUrl, selected.tarball)
}

function isIntegrityAddressedTarball (tarball: unknown, pin: RegistryPin): boolean {
  return typeof tarball === 'string' &&
    isIntegrityAddressedRegistryTarballUrl(normalizeRegistryUrl(tarball), pin.integrity, pin.registry)
}

function checkTarballUrlMatches (lockfileTarball: string, registryTarball: unknown): ResolutionViolation | undefined {
  if (typeof registryTarball === 'string' && sameTarballUrl(lockfileTarball, registryTarball)) return undefined
  return {
    ok: false,
    code: TARBALL_URL_MISMATCH_VIOLATION_CODE,
    reason: typeof registryTarball !== 'string'
      ? UNVERIFIABLE_REASON
      : `has a tarball URL (${lockfileTarball}) that does not match the registry's published metadata (${registryTarball})`,
  }
}

function revisionMismatch (reason: string): ResolutionViolation {
  return { ok: false, code: TARBALL_REVISION_MISMATCH_VIOLATION_CODE, reason }
}

/**
 * The registry's current artifact must have exactly one history entry for its
 * revision, that entry must agree with it, and its tarball URL must be
 * addressed by its own integrity.
 */
function isConsistentCurrentRevision (
  current: RegistryArtifact,
  currentHistory: RegistryArtifact[],
  registry: string
): boolean {
  if (currentHistory.length !== 1) return false
  const [historyEntry] = currentHistory
  return historyEntry.integrity === current.integrity &&
    typeof historyEntry.tarball === 'string' &&
    typeof current.tarball === 'string' &&
    typeof current.integrity === 'string' &&
    isIntegrityAddressedRegistryTarballUrl(normalizeRegistryUrl(current.tarball), current.integrity, registry) &&
    sameTarballUrl(historyEntry.tarball, current.tarball)
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
