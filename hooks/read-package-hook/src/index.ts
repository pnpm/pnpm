import semver from 'semver'

export { createReadPackageHook } from './createReadPackageHook'

export function isSubRange (superRange: string | undefined, subRange: string) {
  return !superRange ||
  subRange === superRange ||
  semver.validRange(subRange) != null &&
  semver.validRange(superRange) != null &&
  semver.subset(subRange, superRange)
}
