import type { RangeSpecGranularity, RangeSpecStyle } from '@pnpm/types'
import semver from 'semver'

import { inferRangeSpecStyle } from './inferRangeSpecStyle.js'

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
 * The existing entry's range style wins over the requested specifier's, which
 * wins over the configured default, so a re-add keeps the pinning style the
 * manifest already used. A newly added prerelease is pinned exactly, while an
 * updated prerelease keeps the existing entry's range style.
 */
export function calcVersionRange (
  version: string,
  opts: {
    prevSpecifier?: string
    bareSpecifier?: string
    defaultRangeSpecStyle?: RangeSpecStyle
  }
): string {
  if (semver.parse(version)?.prerelease.length) {
    const prevRangeSpecStyle = opts.prevSpecifier ? inferRangeSpecStyle(opts.prevSpecifier) : undefined
    return prevRangeSpecStyle ? versionWithRangeSpecStyle(version, prevRangeSpecStyle) : version
  }
  const rangeSpecStyle = (opts.prevSpecifier ? inferRangeSpecStyle(opts.prevSpecifier) : undefined) ??
    (opts.bareSpecifier ? inferRangeSpecStyle(opts.bareSpecifier) : undefined) ??
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
