import type { PinnedVersion } from '@pnpm/types'
import { parseRange } from 'semver-utils'

export function whichVersionIsPinned (spec: string): PinnedVersion | undefined {
  // A catalog reference carries no version pinning of its own; the pinning is
  // defined by the catalog entry it points to. Bail out so a catalog name that
  // happens to look like a version (e.g. "catalog:express4-21") isn't misread
  // as a pinned version.
  if (spec.startsWith('catalog:')) return undefined
  const colonIndex = spec.indexOf(':')
  if (colonIndex !== -1) {
    spec = spec.substring(colonIndex + 1)
  }
  const index = spec.lastIndexOf('@')
  if (index !== -1) {
    spec = spec.slice(index + 1)
  }
  if (spec === '*') return 'none'
  const parsedRange = parseRange(spec)
  if (parsedRange.length !== 1) return undefined
  const versionObject = parsedRange[0]
  switch (versionObject.operator) {
    case '~': return 'minor'
    case '^': return 'major'
    case undefined:
      if (versionObject.patch) return 'patch'
      if (versionObject.minor) return 'minor'
      if (versionObject.major) return 'major'
  }
  return undefined
}
