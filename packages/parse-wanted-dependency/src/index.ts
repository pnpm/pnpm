import validateNpmPackageName from 'validate-npm-package-name'
import fs from 'fs'

interface ParsedWantedDependency {
  alias: string
  pref: string
}

export default function parseWantedDependency (
  rawWantedDependency: string
): Partial<ParsedWantedDependency> & (Omit<ParsedWantedDependency, 'pref'> | Omit<ParsedWantedDependency, 'alias'> | ParsedWantedDependency) {
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
  if (rawWantedDependency.startsWith('link:')) {
    const linkFilePath = rawWantedDependency.slice(5)
    const packageJson = fs.readFileSync(`${linkFilePath}/package.json`, 'utf8')
    const parsedPackageJson = JSON.parse(packageJson)
    return {
      pref: rawWantedDependency,
      alias: parsedPackageJson.name,
    }
  }
  return {
    pref: rawWantedDependency,
  }
}
