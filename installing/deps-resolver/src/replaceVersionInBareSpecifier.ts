import semver from 'semver'

export function replaceVersionInBareSpecifier (bareSpecifier: string, version: string): string {
  if (semver.validRange(bareSpecifier)) {
    return version
  }
  if (!bareSpecifier.startsWith('npm:')) {
    return bareSpecifier
  }
  const versionDelimiter = bareSpecifier.lastIndexOf('@')
  if (versionDelimiter === -1 || bareSpecifier.indexOf('/') > versionDelimiter) {
    return `${bareSpecifier}@${version}`
  }
  return `${bareSpecifier.substring(0, versionDelimiter + 1)}${version}`
}
