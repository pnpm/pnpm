import semver from 'semver'

/**
 * Orders versions for sorting. A local package's lockfile entry may carry no
 * version, which `semver.compare` rejects, so versions that are not valid
 * semver sort lexically before every valid one. Valid versions sort by
 * semver precedence, which keeps the ordering total.
 */
export function compareVersions (version1: string | undefined, version2: string | undefined): number {
  const valid1 = semver.valid(version1)
  const valid2 = semver.valid(version2)
  if (valid1 != null && valid2 != null) return semver.compare(valid1, valid2)
  if (valid1 != null) return 1
  if (valid2 != null) return -1
  return (version1 ?? '').localeCompare(version2 ?? '')
}
