import type { PackageMeta } from '@pnpm/resolving.registry.types'
import {
  EXISTING_VERSION_SELECTOR_WEIGHT,
  type VersionSelectors,
  type VersionSelectorType,
} from '@pnpm/resolving.resolver-base'

import { hasVersionManifest } from './mirrorLayout.js'
import { semverSatisfiesLoose } from './semverLoose.js'

/**
 * An exact preferred version that satisfies the range but is absent from
 * `meta` can only be learned from the registry.
 */
export function cachedMetaMissesPreferredVersion (
  versionRange: string,
  preferredVersionSelectors: VersionSelectors | undefined,
  meta: PackageMeta
): boolean {
  if (preferredVersionSelectors == null) return false
  for (const [selector, value] of Object.entries(preferredVersionSelectors)) {
    if (!isExactVersionSelectorInRange(selector, value, versionRange)) continue
    if (!hasVersionManifest(meta.versions, selector)) return true
  }
  return false
}

function isExactVersionSelectorInRange (
  selector: string,
  value: VersionSelectors[string],
  versionRange: string
): boolean {
  if (selector === versionRange) return false
  return preferredSelectorInfo(value).selectorType === 'version' && semverSatisfiesLoose(selector, versionRange)
}

export function getDominantLockfileVersion (
  versionRange: string,
  preferredVersionSelectors?: VersionSelectors
): string | null {
  if (preferredVersionSelectors == null) return null
  const lockfileVersion = findSoleLockfileVersion(versionRange, preferredVersionSelectors)
  if (lockfileVersion == null) return null
  return lockfileVersionOutweighsOthers({ lockfileVersion, preferredVersionSelectors, versionRange })
    ? lockfileVersion
    : null
}

/**
 * The single version selector carrying the lockfile weight within
 * `versionRange`. `null` when there is none, more than one, or any selector
 * carries a weight the dominance proof cannot reason about.
 */
function findSoleLockfileVersion (
  versionRange: string,
  preferredVersionSelectors: VersionSelectors
): string | null {
  let lockfileVersion: string | null = null
  for (const [selector, value] of Object.entries(preferredVersionSelectors)) {
    if (selector === versionRange) continue
    const selectorInfo = preferredSelectorInfo(value)
    if (!isPositiveSafeWeight(selectorInfo.weight)) return null
    if (!isLockfileVersionSelector(selector, selectorInfo, versionRange)) continue
    if (lockfileVersion != null) return null
    lockfileVersion = selector
  }
  return lockfileVersion
}

function isPositiveSafeWeight (weight: number): boolean {
  return Number.isSafeInteger(weight) && weight > 0
}

function isLockfileVersionSelector (
  selector: string,
  { selectorType, weight }: PreferredSelectorInfo,
  versionRange: string
): boolean {
  return selectorType === 'version' &&
    weight >= EXISTING_VERSION_SELECTOR_WEIGHT &&
    semverSatisfiesLoose(selector, versionRange)
}

interface LockfileDominanceInput {
  lockfileVersion: string
  preferredVersionSelectors: VersionSelectors
  versionRange: string
}

interface LockfileVersionWeights {
  guaranteedLockfileWeight: number
  maximumOtherVersionWeight: number
}

function lockfileVersionOutweighsOthers (input: LockfileDominanceInput): boolean {
  const weights: LockfileVersionWeights = { guaranteedLockfileWeight: 0, maximumOtherVersionWeight: 0 }
  for (const [selector, value] of Object.entries(input.preferredVersionSelectors)) {
    if (selector === input.versionRange) continue
    addSelectorWeight(weights, { input, selector, selectorInfo: preferredSelectorInfo(value) })
    if (
      !Number.isSafeInteger(weights.guaranteedLockfileWeight) ||
      !Number.isSafeInteger(weights.maximumOtherVersionWeight)
    ) return false
  }
  return weights.guaranteedLockfileWeight > weights.maximumOtherVersionWeight
}

function addSelectorWeight (
  weights: LockfileVersionWeights,
  { input: { lockfileVersion, versionRange }, selector, selectorInfo: { selectorType, weight } }: {
    input: LockfileDominanceInput
    selector: string
    selectorInfo: PreferredSelectorInfo
  }
): void {
  switch (selectorType) {
    case 'version':
      if (selector === lockfileVersion) {
        weights.guaranteedLockfileWeight += weight
      } else if (
        weight < EXISTING_VERSION_SELECTOR_WEIGHT &&
        semverSatisfiesLoose(selector, versionRange)
      ) {
        weights.maximumOtherVersionWeight += weight
      }
      break
    case 'range':
      if (semverSatisfiesLoose(lockfileVersion, selector)) {
        weights.guaranteedLockfileWeight += weight
      }
      // Conservatively assume an unseen version can satisfy every preferred
      // range, even when proving range intersection would be more precise.
      weights.maximumOtherVersionWeight += weight
      break
    case 'tag':
      // A registry can move a tag between requests. Do not count its current
      // target toward the lockfile version, and assume all tags could move to
      // the same unseen version.
      weights.maximumOtherVersionWeight += weight
      break
  }
}

interface PreferredSelectorInfo {
  selectorType: VersionSelectorType
  weight: number
}

export function preferredSelectorInfo (
  value: VersionSelectors[string]
): PreferredSelectorInfo {
  return typeof value === 'string'
    ? { selectorType: value, weight: 1 }
    : value
}
