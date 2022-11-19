import semver from 'semver'

export function equalOrSemverEqual (version1: string, version2: string): boolean {
  if (version1 === version2) return true
  try {
    return semver.eq(version1, version2, { loose: true })
  } catch (err) {
    return false
  }
}
