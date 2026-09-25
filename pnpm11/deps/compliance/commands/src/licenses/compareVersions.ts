import semver from 'semver'

/**
 * A local package's lockfile entry may carry no version, which
 * `semver.compare` rejects.
 */
export function compareVersions (version1: string | undefined, version2: string | undefined): number {
  const valid1 = semver.valid(version1)
  const valid2 = semver.valid(version2)
  if (valid1 != null && valid2 != null) return semver.compare(valid1, valid2)
  return (version1 ?? '').localeCompare(version2 ?? '')
}
