import { type PinnedVersion } from '@pnpm/types'
import { parseRange } from 'semver-utils'

export function whichVersionIsPinned (spec: string): PinnedVersion | undefined {
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
