import type { RangeSpecGranularity, RangeSpecStyle } from '@pnpm/types'

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
