import { globalWarn } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import {
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type VersionSelectors,
} from '@pnpm/resolving.resolver-base'

import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import { applyPublishedByPolicy, pickVersionByVersionRange } from './pickPackageFromMeta.js'
import type { ResolveFromNpmContext, ResolveFromNpmOptions } from './resolverTypes.js'

/**
 * The preferred-version selectors to hand the package picker for `pkgName`.
 *
 * When this package is the user's update target (`updateRequested`), the
 * lockfile's contribution to its selectors is removed so the target
 * re-resolves exactly the way a fresh install would after its lockfile
 * entries were deleted. Everything a fresh install applies is preserved:
 * manifest pins, the versions propagated down the dependency chain, and the
 * negative-weight `range` penalties that `pnpm audit --fix` injects to steer
 * resolution away from vulnerable versions.
 */
export function preferredVersionSelectorsFor (
  opts: Pick<ResolveFromNpmOptions, 'updateRequested' | 'preferredVersions'>,
  pkgName: string
): VersionSelectors | undefined {
  const selectors = opts.preferredVersions?.[pkgName]
  if (!opts.updateRequested) return selectors
  return stripLockfileVersionPins(selectors)
}

/**
 * Remove the lockfile-derived part of the selectors: the concrete pins
 * `getPreferredVersionsFromLockfileAndManifests` seeds at
 * `EXISTING_VERSION_SELECTOR_WEIGHT` — added onto the manifest weight when a
 * manifest entry pins the same version, so the lockfile weight is subtracted
 * rather than the selector dropped, leaving the manifest contribution in
 * effect. Selectors a fresh install would also apply (manifest pins,
 * chain-propagated versions, `range`/`tag` selectors) pass through unchanged.
 * Returns `undefined` when nothing remains.
 */
function stripLockfileVersionPins (selectors?: VersionSelectors): VersionSelectors | undefined {
  if (selectors == null) return undefined
  let kept: VersionSelectors | undefined
  for (const [selector, value] of Object.entries(selectors)) {
    let keptValue = value
    if (typeof value !== 'string' && value.selectorType === 'version' && value.weight >= EXISTING_VERSION_SELECTOR_WEIGHT) {
      const manifestWeight = value.weight - EXISTING_VERSION_SELECTOR_WEIGHT
      if (manifestWeight <= 0) continue
      keptValue = { selectorType: 'version', weight: manifestWeight }
    }
    // Null-prototype: selector keys come from manifests and the lockfile,
    // and a dist-tag named `__proto__` is a valid selector key.
    kept ??= Object.create(null) as VersionSelectors
    kept[selector] = keptValue
  }
  return kept
}

/**
 * During a targeted update the picker still honors the preferred versions a
 * fresh install would apply (manifest pins and versions propagated down the
 * dependency chain), so the target can legitimately settle below the highest
 * version its range admits. Surface that once per package: reaching the
 * newer version everywhere is an override's job, not an update's.
 *
 * The baseline for "held back" is the pick with only the non-pin selectors
 * applied — `range`/`tag` selectors such as the `pnpm audit --fix`
 * vulnerability penalties steer the baseline too, so the warning never
 * recommends a version those selectors avoid. The baseline also honors the
 * `publishedBy` maturity cutoff the actual pick applied: a version blocked
 * by `minimumReleaseAge` is not an update the manifests held back, and
 * recommending an override for it would defeat the age gate.
 *
 * The recommended override is scoped to the declared range being resolved
 * (`name@<range>`), so applying it can never violate any consumer's range:
 * only declarations of exactly this range match the selector, and the
 * recommended version satisfies it by construction.
 */
export function warnOnceOnHeldBackUpdate (
  ctx: Pick<ResolveFromNpmContext, 'warnedHeldBackUpdates'>,
  opts: Pick<ResolveFromNpmOptions, 'updateRequested' | 'preferredVersions' | 'publishedBy' | 'publishedByExclude'>,
  spec: RegistryPackageSpec,
  meta: PackageMeta,
  pickedVersion: string
): void {
  if (!opts.updateRequested || spec.type !== 'range') return
  const selectors = preferredVersionSelectorsFor(opts, spec.name)
  if (selectors == null) return
  // `needsFullMetadata` is not this caller's problem: the pick already
  // succeeded on this metadata, which for an abbreviated packument means
  // every version cleared the cutoff, so `meta` is the filtered view.
  const baselineMeta = opts.publishedBy != null
    ? applyPublishedByPolicy(meta, opts.publishedBy, opts.publishedByExclude).meta
    : meta
  const preferred = pickVersionByVersionRange({
    meta: baselineMeta,
    versionRange: spec.fetchSpec,
    preferredVersionSelectors: pickNonPinSelectors(selectors),
  })
  if (preferred == null || preferred === pickedVersion) return
  const key = `${spec.name}@${spec.fetchSpec}:${pickedVersion}<${preferred}`
  if (ctx.warnedHeldBackUpdates.has(key)) return
  ctx.warnedHeldBackUpdates.add(key)
  globalWarn(`"${spec.name}@${spec.fetchSpec}" was updated to ${pickedVersion}, not ${preferred}, to match the version preferred by your manifests and already installed dependencies. To use ${preferred}, add an override to pnpm-workspace.yaml: overrides: { "${spec.name}@${spec.fetchSpec}": "${preferred}" }`)
}

function pickNonPinSelectors (selectors: VersionSelectors): VersionSelectors | undefined {
  let nonPinSelectors: VersionSelectors | undefined
  for (const [selector, value] of Object.entries(selectors)) {
    if ((typeof value === 'string' ? value : value.selectorType) === 'version') continue
    // Null-prototype for the same reason as in `stripLockfileVersionPins`.
    nonPinSelectors ??= Object.create(null) as VersionSelectors
    nonPinSelectors[selector] = value
  }
  return nonPinSelectors
}
