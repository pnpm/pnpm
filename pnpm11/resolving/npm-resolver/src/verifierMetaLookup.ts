import { FULL_META_DIR } from '@pnpm/constants'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'

import type { CreateNpmResolutionVerifierOptions } from './createNpmResolutionVerifier.js'
import type { FetchMetadataFromFromRegistryOptions } from './fetch.js'
import { fetchAttestationPublishedAt } from './fetchAttestationPublishedAt.js'
import {
  fetchAbbreviatedMetadataCached,
  fetchFullMetadataCached,
} from './fetchFullMetadataCached.js'
import { isMalformedMirrorFragmentError } from './metaMirror.js'
import type { PackageMetaCache } from './pickPackage.js'
import { getPkgMetaCacheKey, getPkgMirrorPath, loadMeta } from './pickPackage.js'

type PublishedAtTimeMap = Record<string, string | undefined>

export interface PublishedAtLookupContext {
  fetchOpts: FetchMetadataFromFromRegistryOptions
  getAuthHeaderValueByURI: GetAuthHeader
  cacheDir?: string
  offline: boolean
  /**
   * The `minimumReleaseAge` cutoff converted to a unix-ms epoch. A
   * version with a publish time strictly less than this passes the
   * policy. Used by the abbreviated-metadata shortcut: if the
   * package's last-modified time is older than the cutoff, every
   * version it contains is too.
   */
  cutoffMs: number
  /**
   * Resolver-owned LRU (per-install) keyed via `getPkgMetaCacheKey`
   * (registry + name, with a `:full` suffix for full meta).
   */
  sharedMetaCache?: PackageMetaCache
  /**
   * Per-(registry+name) memo of the abbreviated metadata fetch.
   * Abbreviated is what the resolver populates by default, so on a
   * non-frozen install the conditional GET hits the disk mirror at
   * ~zero cost. Stores only the two fields the shortcut reads —
   * package-level `modified` plus the set of currently-listed version
   * names — so the multi-hundred-KB packument can be GC'd as soon as
   * the fetch returns (the cache only needs to dedupe network/disk
   * round-trips, not full document storage).
   */
  abbreviatedMetaCache: Map<string, Promise<AbbreviatedMetaResult>>
  /**
   * Per-(registry+name+version) memo of the final published-at answer
   * the verifier hands to the policy check. One install verifies each
   * (name, version) pair at most once.
   */
  publishedAtCache: Map<string, Promise<string | undefined>>
  /**
   * Per-(registry+name) memo of the on-disk full-metadata mirror read.
   * One disk read per package regardless of how many versions we
   * verify of it.
   */
  localMetaCache: Map<string, Promise<PublishedAtTimeMap | undefined>>
  /**
   * Per-(registry+name) memo of the full-metadata network fetch — only
   * issued when both the abbreviated-modified shortcut and the
   * attestation endpoint fail to yield a timestamp.
   */
  fullMetaCache: Map<string, Promise<PublishedAtTimeMap | undefined>>
  /**
   * Per-(registry+name) memo of the full packument used by the trust
   * check (history walk for `failIfTrustDowngraded`). Kept separate
   * from `fullMetaCache` because the trust check needs the whole
   * document (`_npmUser`, `dist.attestations` per version) where the
   * age check only needs `time`.
   */
  fullMetaForTrustCache: Map<string, Promise<PackageMeta>>
}

export interface RegistryArtifact {
  revision: unknown
  integrity: unknown
  tarball: unknown
}

export interface RegistryArtifactHistory {
  current: RegistryArtifact
  revisions: RegistryArtifact[]
}

interface AbbreviatedMetaProjection {
  modified?: string
  /** version → current and historical registry artifacts. */
  versionArtifacts?: Map<string, RegistryArtifactHistory>
}

/**
 * Result of an abbreviated-metadata fetch. The fetch error is carried as a
 * value rather than rejected so the per-install cache can hold a single
 * resolved promise — a cached rejection would surface as an unhandled
 * rejection and be shared across every caller of the same key. The
 * tarball-URL check rethrows this error; the age shortcut ignores it and
 * falls back to per-version lookups.
 *
 * Modeled as a discriminated union so a result carries exactly one of `meta`
 * or `error` — never both, never neither.
 */
type AbbreviatedMetaResult =
  | { meta: AbbreviatedMetaProjection, error?: undefined }
  | { error: unknown, meta?: undefined }

/**
 * Per-install dedup of every network/disk fetch the verifier issues.
 * All maps live here so verifying
 * many versions of the same package only pays the disk/network costs
 * once. The on-disk conditional-GET cache is handled inside
 * fetch{Abbreviated,Full}MetadataCached via the resolver's shared
 * mirrors at opts.cacheDir.
 */
export function createPublishedAtLookupContext (
  opts: Pick<CreateNpmResolutionVerifierOptions, 'fetchOpts' | 'getAuthHeaderValueByURI' | 'cacheDir' | 'offline' | 'metaCache'>,
  cutoffMs: number
): PublishedAtLookupContext {
  return {
    fetchOpts: opts.fetchOpts,
    getAuthHeaderValueByURI: opts.getAuthHeaderValueByURI,
    cacheDir: opts.cacheDir,
    offline: opts.offline === true,
    cutoffMs,
    sharedMetaCache: opts.metaCache,
    abbreviatedMetaCache: new Map(),
    publishedAtCache: new Map(),
    localMetaCache: new Map(),
    fullMetaCache: new Map(),
    fullMetaForTrustCache: new Map(),
  }
}

export function fetchFullMetaForTrust (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PackageMeta> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.fullMetaForTrustCache.get(cacheKey)
  if (cachedPromise == null) {
    // Fast path: if the resolver already upgraded to full meta for this
    // (registry, name) during the same install (e.g. minimumReleaseAge
    // active), reuse that document.
    const shared = readSharedMetaForTrust(context.sharedMetaCache, registry, name)
    if (shared != null) {
      cachedPromise = Promise.resolve(projectTrustMeta(shared))
    } else {
      // Don't swallow the fetch rejection here — `runTrustCheck` lets it
      // propagate so the registry's own fetch error aborts the install. The
      // cache still holds the rejected promise so repeat verifier calls for
      // the same (registry, name) within one install don't refetch a
      // known-failing endpoint.
      //
      // The full document — dependency maps, scripts,
      // READMEs for every version — would otherwise stay resident in this
      // map for the entire install, which on multi-thousand-entry
      // workspaces OOMs CI runners with a 2GB heap (see pnpm/pnpm#11860).
      cachedPromise = fetchFullMetadataCached(context.fetchOpts, name, {
        registry,
        authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
        cacheDir: context.cacheDir,
        offline: context.offline,
      }).then(projectTrustMeta)
    }
    context.fullMetaForTrustCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

// Project the full packument to a minimal `PackageMeta`-shaped view
// that exposes only the fields `failIfTrustDowngraded` reads:
//   • `name` and `modified` for error messages and cache keys
//   • `time` for the per-version publish-date walk
//   • `versions[v]._npmUser.trustedPublisher`
//   • `versions[v].dist.attestations.provenance`
// The shape is still a valid `PackageMeta` so the downstream consumer
// doesn't have to special-case it — only the bulk fields (dependency
// graph, scripts, README, etc.) are dropped.
function projectTrustMeta (meta: PackageMeta): PackageMeta {
  const versions: Record<string, PackageInRegistry> = {}
  for (const [version, manifest] of Object.entries(meta.versions ?? {})) {
    if (manifest != null) versions[version] = projectTrustManifest(manifest)
  }
  return {
    name: meta.name,
    'dist-tags': {},
    versions,
    time: meta.time,
    modified: meta.modified,
    etag: meta.etag,
  }
}

function projectTrustManifest (manifest: PackageInRegistry): PackageInRegistry {
  // Drop everything except the trust-evidence fields. `PackageInRegistry.dist`
  // is typed as requiring `shasum` and `tarball`, but the trust check never
  // reads them; cast away the unsoundness so callers see the same nominal
  // shape without the per-version dependency graph / scripts / README bulk
  // carrying through. `_npmUser` is similarly narrowed to just
  // `trustedPublisher` and `approver` — the only sub-fields the trust check
  // inspects — so we don't keep maintainer name/email PII resident in the
  // cache.
  const approver = manifest._npmUser?.approver
  const trustedPublisher = manifest._npmUser?.trustedPublisher
  const provenance = manifest.dist?.attestations?.provenance
  let npmUser: PackageInRegistry['_npmUser'] = undefined
  if (approver) {
    npmUser ||= {}
    npmUser.approver = {}
  }
  if (trustedPublisher) {
    npmUser ||= {}
    npmUser.trustedPublisher = trustedPublisher
  }
  return {
    _npmUser: npmUser,
    dist: provenance != null
      ? { attestations: { provenance } }
      : undefined,
  } as unknown as PackageInRegistry
}

/**
 * Per-(registry, name, version) lookup with a layered fallback:
 *
 * 1. **Abbreviated metadata `modified` shortcut.** This is what the
 *    resolver already fetches by default; it's a small document with
 *    a package-level last-modified time but no per-version timestamps.
 *    If `modified` is older than the policy cutoff, every version in
 *    this package was published at least that long ago — return the
 *    `modified` timestamp as a conservative upper bound and skip the
 *    rest of the chain. Costs one conditional GET that the resolver
 *    has usually already paid for.
 * 2. **On-disk full-metadata mirror.** If a previous verification
 *    populated `FULL_META_DIR`, take the per-version timestamp from
 *    there.
 * 3. **npm attestation endpoint.** Small payload, just this version's
 *    Sigstore-anchored timestamp. Wins on cold cache when the package
 *    was published with provenance.
 * 4. **Full metadata fetch.** Last resort — only paid when the
 *    abbreviated shortcut can't decide, the local full mirror is
 *    cold, and there's no attestation.
 */
export async function fetchPublishedAt (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  const cacheKey = `${registry}\x00${name}\x00${version}`
  let cachedPromise = context.publishedAtCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = resolvePublishedAt(context, registry, name, version)
    context.publishedAtCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

async function resolvePublishedAt (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  const abbreviatedShortcut = await tryAbbreviatedModifiedShortcut(context, registry, name, version)
  if (abbreviatedShortcut != null) return abbreviatedShortcut

  const localTime = await readLocalMetaTime(context, registry, name)
  if (localTime?.[version]) return localTime[version]

  if (!context.offline) {
    const attestationTime = await fetchAttestationPublishedAt(context.fetchOpts, name, version, {
      registry,
      authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
    })
    if (attestationTime != null) return attestationTime
  }

  const fullMetaTime = await fetchFullMetaTime(context, registry, name)
  return fullMetaTime?.[version]
}

/**
 * Returns the abbreviated metadata's `modified` timestamp **iff** it
 * proves the gate would pass — i.e. modified is strictly older than
 * the policy cutoff *and* the pinned version still exists in the
 * package's current versions map.
 *
 * The version check is the fail-closed contract: an unpublished or
 * never-published version must not slip through on the package-level
 * `modified` timestamp. When the version is missing here we fall
 * through to the later layers so the caller eventually surfaces the
 * "version not present in registry manifest" violation.
 *
 * Returns `undefined` otherwise (modified is too recent, the metadata
 * lacks a parseable modified field, the version isn't in the abbreviated
 * form, or the fetch failed) and the caller proceeds with per-version
 * lookups.
 */
async function tryAbbreviatedModifiedShortcut (
  context: PublishedAtLookupContext,
  registry: string,
  name: string,
  version: string
): Promise<string | undefined> {
  const { meta } = await fetchAbbreviatedMeta(context, registry, name)
  const modified = meta?.modified
  if (typeof modified !== 'string') return undefined
  const modifiedMs = Date.parse(modified)
  if (Number.isNaN(modifiedMs)) return undefined
  if (modifiedMs >= context.cutoffMs) return undefined
  if (!meta?.versionArtifacts?.has(version)) return undefined
  return modified
}

export function fetchAbbreviatedMeta (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<AbbreviatedMetaResult> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.abbreviatedMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    // Fast path: the resolver's per-install LRU already holds this
    // packument from its own pickPackage pass — abbreviated or full.
    const shared = readSharedMeta(context.sharedMetaCache, registry, name)
    const sharedProjection = shared != null ? projectSharedMeta(shared) : undefined
    if (sharedProjection != null) {
      cachedPromise = Promise.resolve({ meta: sharedProjection })
    } else {
      cachedPromise = fetchAbbreviatedMetadataCached(context.fetchOpts, name, {
        registry,
        authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
        cacheDir: context.cacheDir,
        offline: context.offline,
      }).then(
        (meta) => ({ meta: projectAbbreviatedMeta(meta) }),
        (error: unknown) => ({ error })
      )
    }
    context.abbreviatedMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

/**
 * `undefined` when the shared packument holds a corrupt mirror fragment, so
 * the lookup falls through to its own fetch, which repairs the mirror.
 */
function projectSharedMeta (meta: PackageMeta): ReturnType<typeof projectAbbreviatedMeta> | undefined {
  try {
    return projectAbbreviatedMeta(meta)
  } catch (err: unknown) {
    if (isMalformedMirrorFragmentError(err)) return undefined
    throw err
  }
}

function readSharedMeta (
  cache: PackageMetaCache | undefined,
  registry: string,
  name: string
): PackageMeta | undefined {
  if (cache == null) return undefined
  // Prefer a full entry — it carries every field the abbreviated form
  // does, plus `time` and per-version trust evidence the trust check
  // needs. The resolver only populates a full key when the install ran
  // with `minimumReleaseAge` configured, otherwise the bare key holds
  // the abbreviated form.
  return readSharedFullMeta(cache, registry, name) ??
    validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, false, false)), name)
}

function readSharedMetaForTrust (
  cache: PackageMetaCache | undefined,
  registry: string,
  name: string
): PackageMeta | undefined {
  if (cache == null) return undefined
  // Abbreviated meta is rejected for the trust check — it lacks
  // per-version `time` and per-version trust evidence.
  return readSharedFullMeta(cache, registry, name)
}

// The resolver keys full metadata as either filtered or unfiltered
// depending on its own `filterMetadata` setting; the verifier doesn't
// know which, and a filtered full packument keeps everything the
// verifier reads (`time`, per-version `_npmUser`, `dist`), so try both.
function readSharedFullMeta (
  cache: PackageMetaCache,
  registry: string,
  name: string
): PackageMeta | undefined {
  return validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, true, false)), name) ??
    validateSharedMeta(cache.get(getPkgMetaCacheKey(registry, name, true, true)), name)
}

// Defensive guard against the resolver's `metaCache` returning an
// unexpected entry. The cache key is registry-qualified (see
// `getPkgMetaCacheKey`), so a package of the same name from another
// registry can't be returned; this name check catches accidental
// returns of a different package (cache corruption, factory misuse)
// rather than silently feeding wrong data to the trust / age check.
function validateSharedMeta (meta: PackageMeta | undefined, name: string): PackageMeta | undefined {
  if (meta == null) return undefined
  if (meta.name !== name) return undefined
  return meta
}

// Project the abbreviated packument down to the few fields the verifier
// actually reads — package-level `modified`, plus a per-version map of
// `dist.tarball` (whose keys double as the version-existence set for the
// `tryAbbreviatedModifiedShortcut` check and the tarball-URL binding). The
// resolver populates the abbreviated mirror with every version's
// dependency / engine / dist info, which can run to hundreds of KB per
// package and accumulate to many GB across a multi-thousand-entry
// lockfile (see pnpm/pnpm#11860). The full document is GC-able as soon as this
// closure returns; only the short tarball-URL strings are retained.
function projectAbbreviatedMeta (meta: PackageMeta): AbbreviatedMetaProjection {
  return {
    modified: meta.modified,
    versionArtifacts: meta.versions ? projectVersionArtifacts(meta.versions) : undefined,
  }
}

function projectVersionArtifacts (
  versions: Record<string, PackageInRegistry>
): Map<string, RegistryArtifactHistory> {
  const versionArtifacts = new Map<string, RegistryArtifactHistory>()
  for (const [version, manifest] of Object.entries(versions)) {
    if (manifest != null) versionArtifacts.set(version, projectArtifactHistory(manifest))
  }
  return versionArtifacts
}

function projectArtifactHistory (manifest: PackageInRegistry): RegistryArtifactHistory {
  return {
    current: {
      revision: manifest.dist?.revision,
      integrity: manifest.dist?.integrity,
      tarball: manifest.dist?.tarball,
    },
    revisions: Array.isArray(manifest.dist?.revisions)
      ? manifest.dist.revisions.map(projectRegistryArtifact)
      : [],
  }
}

function projectRegistryArtifact (artifact: RegistryArtifact): RegistryArtifact {
  return {
    revision: artifact.revision,
    integrity: artifact.integrity,
    tarball: artifact.tarball,
  }
}

function readLocalMetaTime (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  if (!context.cacheDir) return Promise.resolve(undefined)
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.localMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = loadLocalMetaTime(context.cacheDir, registry, name)
    context.localMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}

async function loadLocalMetaTime (
  cacheDir: string,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  const pkgMirror = getPkgMirrorPath(cacheDir, FULL_META_DIR, registry, name)
  const cached = await loadMeta(pkgMirror)
  return cached?.time
}

export function fetchFullMetaTime (
  context: PublishedAtLookupContext,
  registry: string,
  name: string
): Promise<PublishedAtTimeMap | undefined> {
  const cacheKey = `${registry}\x00${name}`
  let cachedPromise = context.fullMetaCache.get(cacheKey)
  if (cachedPromise == null) {
    cachedPromise = fetchFullMetadataCached(context.fetchOpts, name, {
      registry,
      authHeaderValue: context.getAuthHeaderValueByURI(registry, { pkgName: name }),
      cacheDir: context.cacheDir,
      offline: context.offline,
    }).then((meta) => meta.time)
    context.fullMetaCache.set(cacheKey, cachedPromise)
  }
  return cachedPromise
}
