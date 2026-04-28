import semver from 'semver'

export function replaceVersionInBareSpecifier (
  bareSpecifier: string,
  version: string,
  namedRegistryPrefixes: readonly string[] = []
): string {
  if (semver.validRange(bareSpecifier)) {
    return version
  }
  const prefix = ['npm:', ...namedRegistryPrefixes].find((p) => bareSpecifier.startsWith(p))
  if (prefix == null) {
    return bareSpecifier
  }
  // `<alias>:<version_selector>` (paired with a scoped package alias) —
  // replace the whole body. Only reached for named-registry prefixes since
  // bare `npm:<range>` is not a valid specifier.
  if (semver.validRange(bareSpecifier.slice(prefix.length))) {
    return `${prefix}${version}`
  }
  const versionDelimiter = bareSpecifier.lastIndexOf('@')
  if (versionDelimiter === -1 || bareSpecifier.indexOf('/') > versionDelimiter) {
    return `${bareSpecifier}@${version}`
  }
  return `${bareSpecifier.substring(0, versionDelimiter + 1)}${version}`
}
