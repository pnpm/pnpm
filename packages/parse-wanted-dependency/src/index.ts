import validateNpmPackageName from 'validate-npm-package-name'

export interface ParsedWantedDependency {
  alias: string
  bareSpecifier: string
}

export type ParseWantedDependencyResult = Partial<ParsedWantedDependency> &
(
  Omit<ParsedWantedDependency, 'bareSpecifier'>
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
        bareSpecifier: rawWantedDependency.slice(versionDelimiter + 1),
      }
    }
    return {
      bareSpecifier: rawWantedDependency,
    }
  }
  if (validateNpmPackageName(rawWantedDependency).validForOldPackages) {
    return {
      alias: rawWantedDependency,
    }
  }
  return {
    bareSpecifier: rawWantedDependency,
  }
}
