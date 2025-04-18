import { type PinnedVersion } from '@pnpm/types'
import { parseRange } from 'semver-utils'

export function whichVersionIsPinned (spec: string): PinnedVersion | undefined {
  const isWorkspaceProtocol = spec.startsWith('workspace:')
  if (isWorkspaceProtocol) spec = spec.slice('workspace:'.length)
  if (spec === '*') return isWorkspaceProtocol ? 'patch' : 'none'
  if (spec.startsWith('npm:')) {
    const index = spec.lastIndexOf('@')
    spec = spec.slice(index + 1)
  }
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
