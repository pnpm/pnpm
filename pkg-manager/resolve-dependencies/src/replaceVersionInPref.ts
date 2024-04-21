import semver from 'semver'

export function replaceVersionInPref (pref: string, version: string): string {
  if (semver.validRange(pref)) {
    return version
  }
  if (!pref.startsWith('npm:')) {
    return pref
  }
  const versionDelimiter = pref.lastIndexOf('@')
  if (versionDelimiter === -1 || pref.indexOf('/') > versionDelimiter) {
    return `${pref}@${version}`
  }
  return `${pref.substring(0, versionDelimiter + 1)}${version}`
}
