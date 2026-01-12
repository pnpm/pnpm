import { PnpmError } from '@pnpm/error'
import { createMatcher, type Matcher } from '@pnpm/matcher'
import { type PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

export function createPackageVersionPolicy (patterns: string[]): PackageVersionPolicy {
  const rules: VersionPolicyRule[] = []
  for (const pattern of patterns) {
    const parsed = parseVersionPolicyRule(pattern)
    rules.push({ nameMatcher: createMatcher(parsed.packageName), exactVersions: parsed.exactVersions })
  }
  return evaluateVersionPolicy.bind(null, rules)
}

export function expandPackageVersionSpecs (specs: string[]): Set<string> {
  const expandedSpecs = new Set<string>()
  for (const spec of specs) {
    const parsed = parseVersionPolicyRule(spec)
    if (parsed.exactVersions.length === 0) {
      expandedSpecs.add(parsed.packageName)
    } else {
      for (const version of parsed.exactVersions) {
        expandedSpecs.add(`${parsed.packageName}@${version}`)
      }
    }
  }
  return expandedSpecs
}

function evaluateVersionPolicy (rules: VersionPolicyRule[], pkgName: string): boolean | string[] {
  for (const { nameMatcher, exactVersions } of rules) {
    if (!nameMatcher(pkgName)) {
      continue
    }
    if (exactVersions.length === 0) {
      return true
    }
    return exactVersions
  }
  return false
}

interface VersionPolicyRule {
  nameMatcher: Matcher
  exactVersions: string[]
}

interface ParsedVersionPolicyRule {
  packageName: string
  exactVersions: string[]
}

function parseVersionPolicyRule (pattern: string): ParsedVersionPolicyRule {
  const isScoped = pattern.startsWith('@')
  const atIndex = isScoped ? pattern.indexOf('@', 1) : pattern.indexOf('@')

  if (atIndex === -1) {
    return { packageName: pattern, exactVersions: [] }
  }

  const packageName = pattern.slice(0, atIndex)
  const versionsPart = pattern.slice(atIndex + 1)

  // Parse versions separated by ||
  const exactVersions: string[] | null = parseExactVersionsUnion(versionsPart)
  if (exactVersions == null) {
    throw new PnpmError('INVALID_VERSION_UNION',
      `Invalid versions union. Found: "${pattern}". Use exact versions only.`)
  }
  if (packageName.includes('*')) {
    throw new PnpmError('NAME_PATTERN_IN_VERSION_UNION', `Name patterns are not allowed with version unions. Found: "${pattern}"`)
  }

  return {
    packageName,
    exactVersions,
  }
}

function parseExactVersionsUnion (versionsStr: string): string[] | null {
  const versions: string[] = []
  for (const versionRaw of versionsStr.split('||')) {
    const version = semver.valid(versionRaw)
    if (version == null) {
      return null
    }
    versions.push(version)
  }
  return versions
}
