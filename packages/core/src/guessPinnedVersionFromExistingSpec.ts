import { parseRange } from 'semver-utils'

export default function guessPinnedVersionFromExistingSpec (spec: string) {
  if (spec.startsWith('workspace:')) spec = spec.substr('workspace:'.length)
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
  }
  return undefined
}
