import { globalWarn } from '@pnpm/logger'
import { filterPkgMetadataVersions } from '@pnpm/resolving.registry.pkg-metadata-filter'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest } from '@pnpm/types'
import semver from 'semver'

import { getStoreIntegrity } from './getIntegrity.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import {
  pickLowestVersionByVersionRange,
  pickPackageFromMeta,
  type PickPackageFromMetaOptions,
  pickVersionByVersionRange,
} from './pickPackageFromMeta.js'

export interface PickerOptions extends PickPackageFromMetaOptions {
  fallbackPublishedBy?: Date
  pickLowestVersion?: boolean
  includeLatestTag?: boolean
  currentVersion?: string
  ignoreMissingTimeField?: boolean
}

export type PeekManifestFromStore = (opts: {
  id: PkgResolutionId
  integrity: string
  name?: string
  version?: string
}) => Promise<DependencyManifest | undefined>

type PickOne = (targetSpec: RegistryPackageSpec) => PackageInRegistry | null

interface PickCandidates {
  pickerOpts: PickerOptions
  meta: PackageMeta
  spec: RegistryPackageSpec
  pickOne: PickOne
  /**
   * Picks the prerelease channel's dist-tag. It must not fall back to a
   * version that `minimumReleaseAge` rejects, or that version would win over
   * a mature `latest` and hide it.
   */
  pickChannelTag?: PickOne
}

// When includeLatestTag is set, the "latest" dist-tag is added as a candidate
// alongside the requested spec, and the higher-versioned pick wins. An
// installed prerelease also adds the dist-tag that follows its prerelease
// channel, which a lower stable `latest` would otherwise hide.
function runPicker ({ pickerOpts, meta, spec, pickOne, pickChannelTag = pickOne }: PickCandidates): PackageInRegistry | null {
  const currentPkg = pickOne(spec)
  if (!pickerOpts.includeLatestTag) return currentPkg
  const picked = pickMax(pickOne({ ...spec, type: 'tag', fetchSpec: 'latest' }), currentPkg)
  const channelTag = findPrereleaseChannelTag(meta, pickerOpts.currentVersion)
  if (channelTag == null || channelTag === 'latest') return picked
  return pickMax(pickChannelTag({ ...spec, type: 'tag', fetchSpec: channelTag }), picked)
}

/**
 * Returns the dist-tag pointing at the highest published version on the same
 * prerelease channel as `currentVersion`, or `undefined` when
 * `currentVersion` is not a prerelease or no dist-tag follows its channel.
 */
function findPrereleaseChannelTag (meta: PackageMeta, currentVersion: string | undefined): string | undefined {
  const channel = currentVersion == null ? undefined : getPrereleaseChannel(currentVersion)
  if (channel == null) return undefined
  let bestTag: string | undefined
  let bestVersion: string | undefined
  for (const [tag, version] of Object.entries(meta['dist-tags'] ?? {})) {
    if (meta.versions[version] == null || getPrereleaseChannel(version) !== channel) continue
    if (bestVersion == null || semver.gt(version, bestVersion, true)) {
      bestTag = tag
      bestVersion = version
    }
  }
  return bestTag
}

/**
 * The leading run of letters in the first prerelease identifier. Build
 * counters, dates, and commit hashes that follow it do not change the channel.
 */
function getPrereleaseChannel (version: string): string | undefined {
  const firstIdentifier = semver.prerelease(version, true)?.[0]
  if (typeof firstIdentifier !== 'string') return undefined
  let end = 0
  while (end < firstIdentifier.length && isAsciiLetter(firstIdentifier[end])) end++
  return end === 0 ? undefined : firstIdentifier.slice(0, end)
}

function isAsciiLetter (char: string): boolean {
  return (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z')
}

// Returns whichever pick has the higher version, treating null as "no match".
function pickMax (
  firstPick: PackageInRegistry | null,
  secondPick: PackageInRegistry | null
): PackageInRegistry | null {
  if (!firstPick) return secondPick
  if (!secondPick) return firstPick
  return semver.lt(firstPick.version, secondPick.version) ? secondPick : firstPick
}

const pickHighest = pickPackageFromMeta.bind(null, pickVersionByVersionRange)
const pickLowest = pickPackageFromMeta.bind(null, pickLowestVersionByVersionRange)

// Try the selection cutoff first, then the release-age cutoff if time-based
// resolution tightened it. Only the last fallback drops the maturity filter
// so the install layer can report a violation when no mature version matches.
function pickRespectingMinReleaseAge (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  const pickMatureOnly = (targetSpec: RegistryPackageSpec): PackageInRegistry | null => {
    const pickMature = pickerOpts.pickLowestVersion ? pickLowest : pickHighest
    const mature = pickMature(pickerOpts, meta, targetSpec)
    if (mature) return mature
    if (pickerOpts.fallbackPublishedBy && pickerOpts.publishedBy && pickerOpts.fallbackPublishedBy > pickerOpts.publishedBy) {
      return pickLowest({
        ...pickerOpts,
        publishedBy: pickerOpts.fallbackPublishedBy,
      }, meta, targetSpec)
    }
    return null
  }
  return runPicker({
    pickerOpts,
    meta,
    spec,
    pickOne: (targetSpec) => pickMatureOnly(targetSpec) ?? pickLowest({
      preferredVersionSelectors: pickerOpts.preferredVersionSelectors,
    }, meta, targetSpec),
    pickChannelTag: pickMatureOnly,
  })
}

// When minimumReleaseAge is not active: pick by pickLowestVersion preference.
function pickIgnoringReleaseAge (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  const pickVersion = pickerOpts.pickLowestVersion ? pickLowest : pickHighest
  return runPicker({ pickerOpts, meta, spec, pickOne: (targetSpec) => pickVersion(pickerOpts, meta, targetSpec) })
}

// Used in shortcut/fall-through paths: if it fails (including with
// ERR_PNPM_MISSING_TIME), the caller falls through to the next path — e.g.
// the network fetch that can upgrade abbreviated metadata to full.
export function pickMatchingVersionFast (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  return pickerOpts.publishedBy
    ? pickRespectingMinReleaseAge(pickerOpts, spec, meta)
    : pickIgnoringReleaseAge(pickerOpts, spec, meta)
}

// Used at terminal return sites where no further fallback path exists. When
// metadata lacks the per-version `time` field and ignoreMissingTimeField is
// enabled, skip the minimumReleaseAge filter with a warning instead of
// failing hard.
export function pickMatchingVersionFinal (
  pickerOpts: PickerOptions,
  spec: RegistryPackageSpec,
  meta: PackageMeta
): PackageInRegistry | null {
  try {
    return pickMatchingVersionFast(pickerOpts, spec, meta)
  } catch (err: unknown) {
    if (pickerOpts.ignoreMissingTimeField && isMissingTimeError(err)) {
      warnMissingTimeFieldOnce(meta.name, 'minimumReleaseAge')
      return pickMatchingVersionFast({
        ...pickerOpts,
        publishedBy: undefined,
        publishedByExclude: undefined,
      }, spec, meta)
    }
    throw err
  }
}

/**
 * The offline pick: when the pick the preferences already made names a version
 * the store does not hold, the fetcher could only reject it with
 * ERR_PNPM_NO_OFFLINE_TARBALL, so the packument is narrowed to the store-held
 * versions and the pick is redone over those. When nothing store-held
 * satisfies the spec, the unrestricted pick returns so the existing failure
 * surfaces unchanged (https://github.com/pnpm/pnpm/issues/10715).
 *
 * Returns `undefined` when there is nothing to adjust: not offline, no store
 * to check, a spec that names its target outright (an exact version or a tag
 * has no older alternative to fall back to), or no version in the store. The
 * caller then keeps the unrestricted pick with its existing failure modes.
 */
export async function pickVersionFromStore (
  ctx: {
    offline?: boolean
    peekManifestFromStore?: PeekManifestFromStore
  },
  { pickerOpts, spec, meta, pickedPackage }: {
    pickerOpts: PickerOptions
    spec: RegistryPackageSpec
    meta: PackageMeta
    pickedPackage: PackageInRegistry | null
  }
): Promise<PackageInRegistry | undefined> {
  if (ctx.offline !== true || ctx.peekManifestFromStore == null || spec.type !== 'range') {
    return undefined
  }
  const peekManifestFromStore = ctx.peekManifestFromStore
  // Fast path: the pick the preferences already made is installable
  // offline — one store lookup, no scan.
  if (pickedPackage != null && await isVersionInStore(peekManifestFromStore, { name: meta['name'], version: pickedPackage.version, dist: pickedPackage.dist })) {
    return pickedPackage
  }
  const inStore = await findStoreHeldVersions(peekManifestFromStore, meta, spec.fetchSpec)
  if (inStore.size === 0) return undefined
  const narrowedMeta = filterPkgMetadataVersions(meta, (version) => inStore.has(version))
  return pickMatchingVersionFinal(pickerOpts, spec, narrowedMeta) ?? undefined
}

async function findStoreHeldVersions (
  peekManifestFromStore: PeekManifestFromStore,
  meta: PackageMeta,
  versionRange: string
): Promise<Set<string>> {
  const inStore = new Set<string>()
  await Promise.all(Object.keys(meta.versions).map(async (version) => {
    // The range bounds the scan: a packument may list thousands of versions
    // and the pick can only land on one the range admits. Prereleases stay
    // in, because `*` can pick a prerelease that the `latest` tag names.
    if (!semver.satisfies(version, versionRange, { loose: true, includePrerelease: true })) return
    if (await isVersionInStore(peekManifestFromStore, { name: meta['name'], version, dist: meta.versions[version]?.dist })) {
      inStore.add(version)
    }
  }))
  return inStore
}

async function isVersionInStore (
  peekManifestFromStore: PeekManifestFromStore,
  { name, version, dist }: {
    name: string
    version: string
    dist: PackageInRegistry['dist'] | undefined
  }
): Promise<boolean> {
  const integrity = dist == null ? undefined : getStoreIntegrity(dist)
  if (!integrity) return false
  const storeManifest = await peekManifestFromStore({
    id: `${name}@${version}` as PkgResolutionId,
    integrity,
    name,
    version,
  })
  return storeManifest != null
}

function isMissingTimeError (err: unknown): boolean {
  return (
    err != null &&
    typeof err === 'object' &&
    'code' in err &&
    (err as { code: string }).code === 'ERR_PNPM_MISSING_TIME'
  )
}

// Cap the size so long-lived processes (daemons, store servers) can't leak
// memory via this Set as they resolve ever more distinct packages.
const MAX_WARNED_MISSING_TIME = 1024
const warnedMissingTimeFor = new Set<string>()

/**
 * At most one warning per package per check. `minimumReleaseAge` and
 * `trustPolicy` both go dark on the same missing field, so keying by package
 * alone would let whichever check ran first silence the other and leave the
 * user told about only one of the two skips.
 */
export function warnMissingTimeFieldOnce (
  pkgName: string,
  skippedCheck: 'minimumReleaseAge' | 'trustPolicy'
): void {
  // A package name cannot contain ':', so the check name prefix cannot
  // collide with a name that happens to embed it.
  const key = `${skippedCheck}:${pkgName}`
  if (warnedMissingTimeFor.has(key)) return
  if (warnedMissingTimeFor.size >= MAX_WARNED_MISSING_TIME) {
    // Set preserves insertion order, so the first entry is the oldest.
    const oldest = warnedMissingTimeFor.values().next().value
    if (oldest != null) warnedMissingTimeFor.delete(oldest)
  }
  warnedMissingTimeFor.add(key)
  globalWarn(`The metadata of ${pkgName} is missing the "time" field; skipping the ${skippedCheck} check for this package.`)
}
