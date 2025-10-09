import escapeStringRegexp from 'escape-string-regexp'

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

export type VersionMatcher = (pkgName: string, version?: string) => boolean

export function packageNameMatchesExcludeList (patterns: string[], pkgName: string): boolean {
  const parsedPatterns = patterns.map(parseVersionPattern)
  for (const { packagePattern } of parsedPatterns) {
    const nameMatcher = createMatcher(packagePattern)
    if (nameMatcher(pkgName)) {
      return true
    }
  }
  return false
}

export function getExactVersionFromExcludeList (patterns: string[], pkgName: string): string | undefined {
  const parsedPatterns = patterns.map(parseVersionPattern)
  for (const { packagePattern, exactVersion } of parsedPatterns) {
    const nameMatcher = createMatcher(packagePattern)
    if (nameMatcher(pkgName) && exactVersion) {
      return exactVersion
    }
  }
  return undefined
}

function parseVersionPattern (pattern: string): { packagePattern: string, exactVersion?: string } {
  const isScoped = pattern.startsWith('@')

  if (isScoped) {
    const secondAtIndex = pattern.indexOf('@', 1)
    if (secondAtIndex === -1) {
      return { packagePattern: pattern }
    }
    return {
      packagePattern: pattern.slice(0, secondAtIndex),
      exactVersion: pattern.slice(secondAtIndex + 1),
    }
  }

  const atIndex = pattern.indexOf('@')
  if (atIndex === -1) {
    return { packagePattern: pattern }
  }

  const version = pattern.slice(atIndex + 1)

  if (version.match(/^[~^>=<]/)) {
    throw new Error(
      'Semantic version ranges are not supported in minimumReleaseAgeExclude. ' +
      `Found: "${pattern}". Use exact versions only.`
    )
  }

  return {
    packagePattern: pattern.slice(0, atIndex),
    exactVersion: version,
  }
}

export function createVersionMatcher (patterns: string[]): VersionMatcher {
  const parsedPatterns = patterns.map(parseVersionPattern)

  return (pkgName: string, version?: string): boolean => {
    for (const { packagePattern, exactVersion } of parsedPatterns) {
      const nameMatcher = createMatcher(packagePattern)
      if (!nameMatcher(pkgName)) {
        continue
      }

      if (!exactVersion) {
        return true
      }

      if (!version) {
        return false
      }

      if (version === exactVersion) {
        return true
      }
    }

    return false
  }
}
