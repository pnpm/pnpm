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
  // `<prefix>:<version_selector>` paired with a package alias — replace the
  // whole body. Covers both `npm:^1.0.0` and named-registry forms like
  // `gh:^1.0.0`, where the package name comes from the dependency alias.
  if (semver.validRange(bareSpecifier.slice(prefix.length))) {
    return `${prefix}${version}`
  }
  const versionDelimiter = bareSpecifier.lastIndexOf('@')
  if (versionDelimiter === -1 || bareSpecifier.indexOf('/') > versionDelimiter) {
    return `${bareSpecifier}@${version}`
  }
  return `${bareSpecifier.substring(0, versionDelimiter + 1)}${version}`
}
