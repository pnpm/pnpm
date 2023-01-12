import validateNpmPackageName from 'validate-npm-package-name'

export interface ParsedWantedDependency {
  alias: string
  pref: string
}

export type ParseWantedDependencyResult = Partial<ParsedWantedDependency> &
(
  Omit<ParsedWantedDependency, 'pref'>
  | Omit<ParsedWantedDependency, 'alias'>
  | ParsedWantedDependency
)

export function parseWantedDependency (rawWantedDependency: string): ParseWantedDependencyResult {
  const versionDelimiter = rawWantedDependency.indexOf('@', 1) // starting from 1 to skip the @ that marks scope
  if (versionDelimiter !== -1) {
    const alias = rawWantedDependency.slice(0, versionDelimiter)
    if (validateNpmPackageName(alias).validForOldPackages) {
      return {
        alias,
        pref: rawWantedDependency.slice(versionDelimiter + 1),
      }
    }
    return {
      pref: rawWantedDependency,
    }
  }
  if (validateNpmPackageName(rawWantedDependency).validForOldPackages) {
    return {
      alias: rawWantedDependency,
    }
  }
  return {
    pref: rawWantedDependency,
  }
}
