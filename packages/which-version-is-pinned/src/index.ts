import { parseRange } from 'semver-utils'

export default function whichVersionIsPinned (spec: string) {
  const isWorkspaceProtocol = spec.startsWith('workspace:')
  if (isWorkspaceProtocol) spec = spec.slice('workspace:'.length)
  if (spec === '*') return isWorkspaceProtocol ? 'patch' : 'none'
  const parsedRange = parseRange(spec)
  if (parsedRange.length !== 1) return undefined
  const versionObject = parsedRange[0]
  switch (versionObject.operator) {
  case '~': return 'minor'
  case '^': return 'major'
  case undefined:
    if (versionObject.patch) return 'patch'
    if (versionObject.minor) return 'minor'
  }
  return undefined
}
