import type { MetaHeaders } from './metaMirror.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { PickPackageOptions } from './pickRequest.js'

/**
 * Which reads a pick may try before the registry, decided from the request
 * and the policy alone. The reads themselves check the mirror's content:
 * whether it holds the version and whether its pick agrees.
 */
export interface RouteInputs {
  ctx: { offline?: boolean, preferOffline?: boolean }
  spec: Pick<RegistryPackageSpec, 'type' | 'name'>
  opts: Pick<PickPackageOptions, RouteOption>
}

type RouteOption =
  | 'includeLatestTag'
  | 'updateChecksums'
  | 'refreshMetadata'
  | 'pickLowestVersion'
  | 'publishedBy'
  | 'publishedByExclude'
  | 'trustPolicy'

/**
 * Registries that omit `ETag` cannot answer `If-None-Match` with 304, so a
 * warm revalidation downloads the whole packument. A mirror younger than this
 * and stored without an `ETag` is reused for a range the cache can already
 * satisfy. The public npm registry sends `ETag`s, so it keeps conditional
 * revalidation. After this age the mirror is fetched again, which is how a
 * version published in the meantime shows up.
 */
export const UNVALIDATED_MIRROR_MAX_AGE_MS = 5 * 60 * 1000

/** Whether the pick reads the mirror before any other route. */
export function readsMirrorFirst ({ ctx, opts }: RouteInputs): boolean {
  return ctx.offline === true || ctx.preferOffline === true || opts.pickLowestVersion === true
}

/** The mirror reads a pick tries after {@link readsMirrorFirst}, in order. */
export interface MirrorShortcuts {
  /** A pinned version the mirror holds. */
  exactVersion: boolean
  /** A range whose lockfile version dominates every other selector. */
  dominantLockfileVersion: boolean
  /**
   * A mirror stored without an `ETag`, while
   * {@link isFreshUnvalidatedMirror} holds.
   */
  unvalidatedMirror: boolean
  /**
   * Whether `minimumReleaseAge` applies to the package, when deciding the
   * shortcuts asked. Hand it to {@link mirrorWrittenAfter}, so the policy is
   * asked once per pick.
   */
  releaseAgeApplies?: boolean
}

export function decideMirrorShortcuts (inputs: RouteInputs): MirrorShortcuts {
  const { spec, opts } = inputs
  const releaseAgeApplies = isRangePickWithoutFreshMetadataOptions(inputs) ? releaseAgeAppliesTo(inputs) : undefined
  const stableRange = releaseAgeApplies === false
  return {
    exactVersion: spec.type === 'version' && !opts.includeLatestTag && !opts.updateChecksums,
    dominantLockfileVersion: stableRange,
    unvalidatedMirror: stableRange && opts.refreshMetadata !== true,
    releaseAgeApplies,
  }
}

/**
 * Whether a range pick could be settled from the mirror at all: every option
 * that makes the answer depend on fresh metadata rules it out.
 */
function isStableRangePick (inputs: RouteInputs): boolean {
  // Last: the release-age exclusion is a policy lookup.
  return isRangePickWithoutFreshMetadataOptions(inputs) && !releaseAgeAppliesTo(inputs)
}

function isRangePickWithoutFreshMetadataOptions ({ ctx, spec, opts }: RouteInputs): boolean {
  return (
    spec.type === 'range' &&
    ctx.offline !== true &&
    ctx.preferOffline !== true &&
    !opts.includeLatestTag &&
    !opts.updateChecksums &&
    opts.trustPolicy !== 'no-downgrade'
  )
}

function releaseAgeAppliesTo ({ spec, opts }: RouteInputs): boolean {
  return opts.publishedBy != null && opts.publishedByExclude?.(spec.name) !== true
}

/**
 * The `minimumReleaseAge` cutoff when a mirror written at or after it may
 * answer the pick: such a mirror cannot be missing a mature version. The last
 * mirror read, asked only after the {@link MirrorShortcuts} missed.
 * `releaseAgeApplies` is {@link MirrorShortcuts.releaseAgeApplies}; the
 * exclusion policy is asked only when it is `undefined`.
 */
export function mirrorWrittenAfter (
  inputs: RouteInputs,
  releaseAgeApplies: boolean = releaseAgeAppliesTo(inputs)
): Date | undefined {
  return releaseAgeApplies ? inputs.opts.publishedBy : undefined
}

/**
 * Whether the mirror's header line allows
 * {@link MirrorShortcuts.unvalidatedMirror}.
 */
export function isMirrorWithoutEtag (headers: MetaHeaders | null): boolean {
  return (
    headers != null &&
    (headers.etag == null || headers.etag === '') &&
    headers.uncacheable !== true
  )
}

/**
 * Whether a mirror written at `mtime` is young enough for
 * {@link MirrorShortcuts.unvalidatedMirror}.
 *
 * The age is compared in both directions. A mirror dated far in the future,
 * for example after the clock was set back, has an unknown age and is not
 * reused. A few milliseconds of skew between the file system and the clock
 * are tolerated.
 */
export function isFreshUnvalidatedMirror (mtime: Date, now: number): boolean {
  return Math.abs(now - mtime.getTime()) < UNVALIDATED_MIRROR_MAX_AGE_MS
}

/**
 * What a pick from an in-memory entry must pass before it is served:
 * - `serve`.
 * - `stableRangePick`: serve only the dominant lockfile version.
 * - `revalidate`: fall through to the reads and the fetch that replace the
 *   entry.
 */
export type CacheHitCheck = 'serve' | 'stableRangePick' | 'revalidate'

/**
 * `verified` is false for an entry promoted from the mirror without a
 * registry round-trip. `picked` is whether the entry produced a pick.
 * Offline, every pick is served: there is nothing to revalidate against.
 */
export function decideCacheHitCheck (inputs: RouteInputs, verified: boolean, picked: boolean): CacheHitCheck {
  const { ctx, spec, opts } = inputs
  if (ctx.offline === true || verified) return 'serve'
  if (!picked) return 'revalidate'
  // These picks answer the same from a stale mirror.
  if (ctx.preferOffline === true || opts.pickLowestVersion === true || spec.type === 'version') return 'serve'
  return isStableRangePick(inputs) ? 'stableRangePick' : 'revalidate'
}
