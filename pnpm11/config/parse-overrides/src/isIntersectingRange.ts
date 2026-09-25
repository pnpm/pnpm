import semver from 'semver'

/**
 * Whether a dependency's bare specifier (range2) matches an override
 * selector's version constraint (range1). An absent constraint matches any
 * specifier. Otherwise the two must be identical strings or intersecting
 * semver ranges. Non-semver specifiers only match a constraint they equal.
 */
export function isIntersectingRange (range1: string | undefined, range2: string): boolean {
  return !range1 ||
    range2 === range1 ||
    (semver.validRange(range2) != null &&
      semver.validRange(range1) != null &&
      semver.intersects(range2, range1))
}
