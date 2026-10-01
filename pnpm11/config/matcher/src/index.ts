import escapeStringRegexp from 'escape-string-regexp'

export type Matcher = (input: string) => boolean
export type MatcherWithIndex = (input: string) => number

export function createMatcher (patterns: string[] | string): Matcher {
  const matchWithIndex = createMatcherWithIndex(Array.isArray(patterns) ? patterns : [patterns])
  return (input) => matchWithIndex(input) !== -1
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
  for (let patternIndex = 0; patternIndex < matchArr.length; patternIndex++) {
    if (matchArr[patternIndex].match(input)) return patternIndex
  }
  return -1
}

function matchInputWithoutIgnoreMatchers (matchArr: MatcherFunction[], input: string): number {
  return matchArr.some(({ match }) => match(input)) ? -1 : 0
}

function matchInputWithMatchersArray (matchArr: MatcherFunction[], input: string): number {
  let matchedPatternIndex = -1
  for (let patternIndex = 0; patternIndex < matchArr.length; patternIndex++) {
    const { ignore, match } = matchArr[patternIndex]
    if (ignore) {
      if (match(input)) {
        matchedPatternIndex = -1
      }
    } else if (matchedPatternIndex === -1 && match(input)) {
      matchedPatternIndex = patternIndex
    }
  }
  return matchedPatternIndex
}

function matcherFromPattern (pattern: string): Matcher {
  if (pattern === '*') {
    return () => true
  }

  const escapedPattern = escapeStringRegexp(pattern)
    .replace(/\\\*/g, '.*')
    .replace(/\\\?/g, '.')
  if (escapedPattern === pattern) {
    return (input: string) => input === pattern
  }

  const regexp = new RegExp(`^${escapedPattern}$`, 'su')
  return (input: string) => regexp.test(input)
}

function isIgnorePattern (pattern: string): boolean {
  return pattern[0] === '!'
}

function matcherWhenOnlyOnePatternWithIndex (pattern: string): MatcherWithIndex {
  const match = matcherWhenOnlyOnePattern(pattern)
  return (input) => match(input) ? 0 : -1
}

function matcherWhenOnlyOnePattern (pattern: string): Matcher {
  if (!isIgnorePattern(pattern)) {
    return matcherFromPattern(pattern)
  }
  const ignorePattern = pattern.substring(1)
  const matchIgnored = matcherFromPattern(ignorePattern)
  return (input) => !matchIgnored(input)
}
