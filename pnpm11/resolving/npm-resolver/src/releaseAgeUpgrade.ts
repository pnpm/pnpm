import type { PackageMeta } from '@pnpm/resolving.registry.types'

import type { FetchMetadataNotModifiedResult, FetchMetadataResult } from './fetch.js'
import { condenseMetaForCache, encodeMirror, saveMetaBestEffort } from './metaMirror.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { PickPackageFromMetaOptions } from './pickPackageFromMeta.js'

export type FetchPackageMeta = (pkgName: string, opts: {
  registry: string
  authHeaderValue?: string
  cacheBypass?: boolean
  fullMetadata?: boolean
  etag?: string
  modified?: string
}) => Promise<FetchMetadataResult | FetchMetadataNotModifiedResult>

export interface ReleaseAgeUpgrade {
  meta: PackageMeta
  upgradedFrom?: FetchMetadataResult
}

// When `minimumReleaseAge` is active and we have abbreviated metadata (which
// the npm registry serves by default and which omits per-version `time`),
// the maturity check can't run on the data we have. If the package has been
// modified since the maturity cutoff, re-fetch with `fullMetadata: true` so
// `time` is populated and the check can proceed properly. Without this,
// `pickMatchingVersionFinal` would fall back to its warn-and-skip path,
// silently bypassing the minimumReleaseAge guarantee for affected packages.
//
// Returns the original meta when no upgrade is needed. When an upgrade
// happens, returns both the upgraded meta and the underlying fetch result
// so callers can persist it to disk and avoid re-fetching on next install.
export async function maybeUpgradeAbbreviatedMetaForReleaseAge (
  ctx: {
    fetch: FetchPackageMeta
    offline?: boolean
    releaseAgeUpgradeCheckedPackuments?: WeakSet<PackageMeta>
  },
  spec: RegistryPackageSpec,
  opts: {
    publishedBy?: Date
    publishedByExclude?: PickPackageFromMetaOptions['publishedByExclude']
    authHeaderValue?: string
    registry: string
  },
  meta: PackageMeta
): Promise<ReleaseAgeUpgrade> {
  if (
    ctx.offline === true ||
    !opts.publishedBy ||
    meta.time != null ||
    ctx.releaseAgeUpgradeCheckedPackuments?.has(meta) === true ||
    opts.publishedByExclude?.(spec.name) === true
  ) {
    return { meta }
  }
  const modifiedDate = parseModifiedDate(meta)
  if (modifiedDate != null && modifiedDate <= opts.publishedBy) {
    // The package was last modified at or before the maturity cutoff. Since
    // `modified` is an upper bound on every version's publish time, no version
    // can be newer than the cutoff, so the abbreviated form is fine.
    // Inclusive at the boundary on purpose: matches the per-version `<=` filter
    // in `filterPkgMetadataByPublishDate`.
    return { meta }
  }
  // When `modified` is missing or malformed we fall through to the upgrade
  // fetch: prefer correctness (run the maturity check on real `time` data)
  // over saving a network call when our cached freshness signal is unusable.
  // An ETag and a `Last-Modified` date describe one representation, and `meta`
  // holds the abbreviated one. A registry that reuses them across both forms
  // answers 304, leaving the maturity check without per-version publish dates.
  let fullFetchResult: FetchMetadataResult | FetchMetadataNotModifiedResult
  try {
    fullFetchResult = await ctx.fetch(spec.name, {
      authHeaderValue: opts.authHeaderValue,
      fullMetadata: true,
      registry: opts.registry,
    })
  } catch (err: unknown) {
    // The registry declined to hand over a body. Since the request carries no
    // validators, it says so by repeating an unsolicited 304 until the fetcher
    // gives up, which throws instead of reporting `notModified`.
    if (!isNotModifiedWithoutCacheError(err)) throw err
    ctx.releaseAgeUpgradeCheckedPackuments?.add(meta)
    return { meta }
  }
  if (fullFetchResult.notModified) {
    // The registry has no fuller form of this document. An upgrade that cannot
    // happen must not fail an install that would otherwise succeed: the
    // maturity check falls back to the warn-or-error gate
    // `minimumReleaseAgeIgnoreMissingTime` already governs.
    ctx.releaseAgeUpgradeCheckedPackuments?.add(meta)
    return { meta }
  }
  return { meta: fullFetchResult.meta, upgradedFrom: fullFetchResult }
}

/**
 * The packument's `modified` date, or `null` when it is missing or malformed.
 */
export function parseModifiedDate (meta: PackageMeta): Date | null {
  const modifiedDate = meta.modified ? new Date(meta.modified) : null
  return modifiedDate != null && !Number.isNaN(modifiedDate.getTime()) ? modifiedDate : null
}

function isNotModifiedWithoutCacheError (err: unknown): boolean {
  return (
    err != null &&
    typeof err === 'object' &&
    'code' in err &&
    (err as { code: string }).code === 'ERR_PNPM_META_NOT_MODIFIED_WITHOUT_CACHE'
  )
}

/**
 * The document to serve and cache after an upgrade attempt, marked so no
 * later pick in this resolver repeats the request.
 *
 * Marking belongs here rather than in
 * {@link maybeUpgradeAbbreviatedMetaForReleaseAge} because persisting the
 * response to the mirror can hand back a different object, and only the one
 * that reaches the cache is worth remembering. A registry whose full form is
 * no more complete than its abbreviated one still answers `200`, so a
 * successful upgrade has to be marked too — otherwise every dependency edge
 * re-asks for the same full document.
 */
export function upgradeMetaForCache (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean, releaseAgeUpgradeCheckedPackuments?: WeakSet<PackageMeta> },
  upgrade: ReleaseAgeUpgrade,
  opts: { pkgMirror: string, dryRun: boolean }
): PackageMeta {
  if (upgrade.upgradedFrom == null) return upgrade.meta
  const meta = opts.dryRun
    ? condenseMetaForCache(ctx, upgrade.meta)
    : persistUpgradedMeta(ctx, opts.pkgMirror, upgrade.upgradedFrom)
  ctx.releaseAgeUpgradeCheckedPackuments?.add(meta)
  return meta
}

// A condensing resolver keeps and mirrors the condensed form — the mirror
// only has to carry `time` into the next install; otherwise the unstripped
// meta is written and kept. Either way the abbreviated
// mirror now holds the full document, so its ETag is recorded as `fullEtag`.
function persistUpgradedMeta (
  ctx: { fullMetadata?: boolean, filterMetadata?: boolean },
  pkgMirror: string,
  upgradedFrom: FetchMetadataResult
): PackageMeta {
  const metaForCache = condenseMetaForCache(ctx, upgradedFrom.meta)
  const fullEtag = upgradedFrom.etag
  const content = encodeMirror(ctx, upgradedFrom, {
    meta: metaForCache,
    etag: undefined,
    body: { uncacheable: upgradedFrom.uncacheable, fullEtag },
  })
  saveMetaBestEffort(pkgMirror, content, upgradedFrom.uncacheable === true)
  return metaForCache
}
