import semver from 'semver'

export function isSubRange (superRange: string | undefined, subRange: string | undefined): boolean {
  if (!superRange) return true
  if (!subRange) return false
  if (subRange === superRange) return true
  if (semver.validRange(subRange) == null || semver.validRange(superRange) == null) {
    return false
  }
  const exactSubVersion = semver.clean(subRange)
  if (exactSubVersion != null) {
    return semver.satisfies(exactSubVersion, superRange)
  }
  return semver.subset(subRange, superRange)
}
