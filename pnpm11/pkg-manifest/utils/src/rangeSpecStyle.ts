import type { RangeSpecGranularity, RangeSpecStyle } from '@pnpm/types'
import semver from 'semver'

import { getRangeOfSpecifier, inferRangeSpecStyle } from './inferRangeSpecStyle.js'

export function rangeSpecGranularity (style: RangeSpecStyle): RangeSpecGranularity {
  return style === 'exact' ? 'patch' : style
}

/**
 * Interpret the `save-exact` and `save-prefix` settings into a
 * {@link RangeSpecStyle}. `save-exact` (like the empty `save-prefix`) wins
 * and saves the bare version; a `save-prefix` of `=` saves the version with
 * an explicit `=` operator.
 */
export function getRangeSpecStyle (opts: { saveExact?: boolean, savePrefix?: string }): RangeSpecStyle {
  if (opts.saveExact === true || opts.savePrefix === '') return 'patch'
  switch (opts.savePrefix) {
    case '=': return 'exact'
    case '~': return 'minor'
    default: return 'major'
  }
}

/**
 * The manifest range that pins `version` for a dependency the user asked for
 * as `bareSpecifier` and whose manifest entry, if it already had one, read
 * `prevSpecifier`.
 *
 * The requested specifier's range style wins over the existing entry's, which
 * wins over the configured default, so an explicit version or range requested
 * by the user is honored (pnpm/pnpm#6040), while a request naming no range
 * style (like `latest`) keeps the pinning style the manifest already used.
 * A newly added prerelease is pinned exactly, while an updated prerelease
 * keeps the existing entry's range style.
 *
 * An existing range in a shape no style describes (`<= 3.0.0`, `>=1 <2`,
 * `1 || 2`) is kept as written when it still admits `version` and the request
 * names no specifier of its own, so an update moves the version without
 * trading the range's bounds for the default prefix (pnpm/pnpm#6714). A request
 * that names one (`pnpm add foo@1.2.3`) is the range it wants instead.
 */
export function calcVersionRange (
  version: string,
  opts: {
    prevSpecifier?: string
    bareSpecifier?: string
    defaultRangeSpecStyle?: RangeSpecStyle
  }
): string {
  const prevRangeSpecStyle = opts.prevSpecifier ? inferRangeSpecStyle(opts.prevSpecifier) : undefined
  if (prevRangeSpecStyle == null && opts.prevSpecifier && (opts.bareSpecifier == null || opts.bareSpecifier === opts.prevSpecifier)) {
    const prevRange = getRangeOfSpecifier(opts.prevSpecifier)
    if (prevRange != null && semver.validRange(prevRange) != null && semver.satisfies(version, prevRange)) {
      return prevRange
    }
  }
  if (semver.parse(version)?.prerelease.length) {
    return prevRangeSpecStyle ? versionWithRangeSpecStyle(version, prevRangeSpecStyle) : version
  }
  const requestedRangeSpecStyle = opts.bareSpecifier ? inferRangeSpecStyle(opts.bareSpecifier) : undefined
  const rangeSpecStyle = requestedRangeSpecStyle ??
    prevRangeSpecStyle ??
    opts.defaultRangeSpecStyle
  return versionWithRangeSpecStyle(version, rangeSpecStyle ?? 'major')
}

export function versionWithRangeSpecStyle (version: string, rangeSpecStyle: RangeSpecStyle): string {
  switch (rangeSpecStyle) {
    case 'none':
    case 'major': return `^${version}`
    case 'minor': return `~${version}`
    case 'patch': return version
    case 'exact': return `=${version}`
  }
  // Unreachable for type-checked callers; fail loudly for untyped JS input.
  throw new Error(`Unknown range spec style: '${String(rangeSpecStyle)}'`)
}
