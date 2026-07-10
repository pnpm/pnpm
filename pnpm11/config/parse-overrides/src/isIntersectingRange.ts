import semver from 'semver'

/**
 * Whether a dependency's bare specifier (`range2`) is matched by an override
 * selector's version constraint (`range1`): an absent constraint matches any
 * specifier, otherwise the two must be identical strings or intersecting
 * semver ranges. Non-semver specifiers (`file:`, `catalog:`, git URLs, …)
 * only match a constraint they equal verbatim.
 */
export function isIntersectingRange (range1: string | undefined, range2: string): boolean {
  return !range1 ||
    range2 === range1 ||
    semver.validRange(range2) != null &&
    semver.validRange(range1) != null &&
    semver.intersects(range2, range1)
}
