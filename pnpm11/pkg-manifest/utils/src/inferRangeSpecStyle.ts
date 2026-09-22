import type { RangeSpecStyle } from '@pnpm/types'
import { parseRange } from 'semver-utils'

export function inferRangeSpecStyle (spec: string): RangeSpecStyle | undefined {
  const range = getRangeOfSpecifier(spec)
  if (range == null) return undefined
  if (range === '*') return 'none'
  spec = range
  const parsedRange = parseRange(spec)
  if (parsedRange.length !== 1) return undefined
  const versionObject = parsedRange[0]
  switch (versionObject.operator) {
    case '~': return 'minor'
    case '^': return 'major'
    // A bare '=' before a full version is an explicit exact pin; a partial
    // '=' pins the same way the plain version it prefixes does.
    case '=':
    case undefined:
      if (versionObject.patch) return versionObject.operator === '=' ? 'exact' : 'patch'
      if (versionObject.minor) return 'minor'
      if (versionObject.major) return 'major'
  }
  return undefined
}

/**
 * The range a specifier declares once its protocol prefix (`npm:`, `jsr:`,
 * `workspace:`, ...) and alias (`foo@`, `@scope/foo@`) are stripped.
 * `undefined` for a `catalog:` reference, which pins nothing of its own: the
 * pinning is defined by the catalog entry it points to, so a catalog name that
 * happens to look like a version (e.g. "catalog:express4-21") must not be read
 * as one.
 */
export function getRangeOfSpecifier (spec: string): string | undefined {
  if (spec.startsWith('catalog:')) return undefined
  const colonIndex = spec.indexOf(':')
  if (colonIndex !== -1) {
    spec = spec.substring(colonIndex + 1)
  }
  const index = spec.lastIndexOf('@')
  if (index !== -1) {
    spec = spec.slice(index + 1)
  }
  return spec
}
