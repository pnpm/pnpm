import type { PackageMeta } from '@pnpm/resolving.registry.types'
import {
  RESOLVED_VERSION_SELECTOR_WEIGHT,
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

/**
 * The exact selector a pick for `versionRange` is proven to land on, whatever
 * the registry publishes after the packument was cached. Each exact selector
 * that satisfies the range is guaranteed its own weight plus every preferred
 * range it satisfies. The heaviest must exceed what the range and tag
 * selectors could give any other version, and every other exact selector must
 * stay below it even with every tag's weight added, so a tie never qualifies.
 * `null` when no exact selector satisfies the range, a weight is not a
 * positive safe integer, or the proof does not hold.
 */
export function getDominantLockfileVersion (
  versionRange: string,
  preferredVersionSelectors?: VersionSelectors
): string | null {
  if (preferredVersionSelectors == null) return null
  const weights = collectSelectorWeights(versionRange, preferredVersionSelectors)
  if (weights == null) return null
  const exact = weights.exact.map(([version, weight]): [string, number] => [
    version,
    weight + sumWeights(weights.ranges.filter(([range]) => semverSatisfiesLoose(version, range))),
  ])
  const movable = weights.tags + sumWeights(weights.ranges)
  const winner = exact.reduce<[string, number] | null>((best, entry) => (best == null || entry[1] > best[1]) ? entry : best, null)
  return winner != null && isProvenWinner(winner, { exact, movable, tags: weights.tags }) ? winner[0] : null
}

interface SelectorWeights {
  exact: Array<[string, number]>
  ranges: Array<[string, number]>
  tags: number
}

function collectSelectorWeights (versionRange: string, selectors: VersionSelectors): SelectorWeights | null {
  const weights: SelectorWeights = { exact: [], ranges: [], tags: 0 }
  for (const [selector, value] of Object.entries(selectors)) {
    if (selector === versionRange) continue
    const info = preferredSelectorInfo(value)
    if (!isPositiveSafeWeight(info.weight)) return null
    addSelectorWeight(weights, { info, selector, versionRange })
  }
  return weights
}

function addSelectorWeight (
  weights: SelectorWeights,
  { info: { selectorType, weight }, selector, versionRange }: { info: PreferredSelectorInfo, selector: string, versionRange: string }
): void {
  switch (selectorType) {
    case 'version':
      if (semverSatisfiesLoose(selector, versionRange)) weights.exact.push([selector, weight])
      break
    case 'range':
      weights.ranges.push([selector, weight])
      break
    case 'tag':
      weights.tags += weight
      break
  }
}

function isProvenWinner (
  [winner, top]: [string, number],
  { exact, movable, tags }: { exact: Array<[string, number]>, movable: number, tags: number }
): boolean {
  if (!Number.isSafeInteger(top) || !Number.isSafeInteger(movable) || top <= movable) return false
  return exact.every(([version, weight]) => version === winner || weight + tags < top)
}

function sumWeights (entries: Array<[string, number]>): number {
  return entries.reduce((sum, [, weight]) => sum + weight, 0)
}

function isPositiveSafeWeight (weight: number): boolean {
  return Number.isSafeInteger(weight) && weight > 0
}

interface PreferredSelectorInfo {
  selectorType: VersionSelectorType
  weight: number
}

export function preferredSelectorInfo (
  value: VersionSelectors[string]
): PreferredSelectorInfo {
  return typeof value === 'string'
    ? { selectorType: value, weight: RESOLVED_VERSION_SELECTOR_WEIGHT }
    : value
}
