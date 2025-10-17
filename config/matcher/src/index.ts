import { PnpmError } from '@pnpm/error'
import { type PackageVersionPolicy } from '@pnpm/types'
import escapeStringRegexp from 'escape-string-regexp'
import semver from 'semver'

type Matcher = (input: string) => boolean
type MatcherWithIndex = (input: string) => number

export function createMatcher (patterns: string[] | string): Matcher {
  const m = createMatcherWithIndex(Array.isArray(patterns) ? patterns : [patterns])
  return (input) => m(input) !== -1
}

interface MatcherFunction {
  match: Matcher
  ignore: boolean
}

export function createMatcherWithIndex (patterns: string[]): MatcherWithIndex {
  switch (patterns.length) {
  case 0: return () => -1
  case 1: return matcherWhenOnlyOnePatternWithIndex(patterns[0])
  }
  const matchArr: MatcherFunction[] = []
  let hasIgnore = false
  let hasInclude = false
  for (const pattern of patterns) {
    if (isIgnorePattern(pattern)) {
      hasIgnore = true
      matchArr.push({ ignore: true, match: matcherFromPattern(pattern.substring(1)) })
    } else {
      hasInclude = true
      matchArr.push({ ignore: false, match: matcherFromPattern(pattern) })
    }
  }
  if (!hasIgnore) {
    return matchInputWithNonIgnoreMatchers.bind(null, matchArr)
  }
  if (!hasInclude) {
    return matchInputWithoutIgnoreMatchers.bind(null, matchArr)
  }
  return matchInputWithMatchersArray.bind(null, matchArr)
}

function matchInputWithNonIgnoreMatchers (matchArr: MatcherFunction[], input: string): number {
  for (let i = 0; i < matchArr.length; i++) {
    if (matchArr[i].match(input)) return i
  }
  return -1
}

function matchInputWithoutIgnoreMatchers (matchArr: MatcherFunction[], input: string): number {
  return matchArr.some(({ match }) => match(input)) ? -1 : 0
}

function matchInputWithMatchersArray (matchArr: MatcherFunction[], input: string): number {
  let matchedPatternIndex = -1
  for (let i = 0; i < matchArr.length; i++) {
    const { ignore, match } = matchArr[i]
    if (ignore) {
      if (match(input)) {
        matchedPatternIndex = -1
      }
    } else if (matchedPatternIndex === -1 && match(input)) {
      matchedPatternIndex = i
    }
  }
  return matchedPatternIndex
}

function matcherFromPattern (pattern: string): Matcher {
  if (pattern === '*') {
    return () => true
  }

  const escapedPattern = escapeStringRegexp(pattern).replace(/\\\*/g, '.*')
  if (escapedPattern === pattern) {
    return (input: string) => input === pattern
  }

  const regexp = new RegExp(`^${escapedPattern}$`)
  return (input: string) => regexp.test(input)
}

function isIgnorePattern (pattern: string): boolean {
  return pattern[0] === '!'
}

function matcherWhenOnlyOnePatternWithIndex (pattern: string): MatcherWithIndex {
  const m = matcherWhenOnlyOnePattern(pattern)
  return (input) => m(input) ? 0 : -1
}

function matcherWhenOnlyOnePattern (pattern: string): Matcher {
  if (!isIgnorePattern(pattern)) {
    return matcherFromPattern(pattern)
  }
  const ignorePattern = pattern.substring(1)
  const m = matcherFromPattern(ignorePattern)
  return (input) => !m(input)
}

export function createPackageVersionPolicy (patterns: string[]): PackageVersionPolicy {
  const rules = patterns.map(parseVersionPolicyRule)
  return evaluateVersionPolicy.bind(null, rules)
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

function parseVersionPolicyRule (pattern: string): VersionPolicyRule {
  const isScoped = pattern.startsWith('@')
  const atIndex = isScoped ? pattern.indexOf('@', 1) : pattern.indexOf('@')

  if (atIndex === -1) {
    return { nameMatcher: createMatcher(pattern), exactVersions: [] }
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
    nameMatcher: (pkgName: string) => pkgName === packageName,
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
