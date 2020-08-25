import validateNpmPackageName = require('validate-npm-package-name')

interface ParsedWantedDependency {
  alias: string
  pref: string
}

export default function parseWantedDependency (
  rawWantedDependency: string
): Partial<ParsedWantedDependency> & (Omit<ParsedWantedDependency, 'pref'> | Omit<ParsedWantedDependency, 'alias'> | ParsedWantedDependency) {
  const versionDelimiter = rawWantedDependency.indexOf('@', 1) // starting from 1 to skip the @ that marks scope
  if (versionDelimiter !== -1) {
    const alias = rawWantedDependency.substr(0, versionDelimiter)
    if (validateNpmPackageName(alias).validForOldPackages) {
      return {
        alias,
        pref: rawWantedDependency.substr(versionDelimiter + 1),
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
