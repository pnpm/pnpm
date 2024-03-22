import '@total-typescript/ts-reset'

import escapeStringRegexp from 'escape-string-regexp'

type Matcher = (input: string) => boolean
type MatcherWithIndex = (input: string) => number

export function createMatcher(patterns: string[] | string): Matcher {
  const m = createMatcherWithIndex(
    Array.isArray(patterns) ? patterns : [patterns]
  )

  return (input) => m(input) !== -1
}

type MatcherFunction = {
  match: Matcher
  ignore: boolean
}

export function createMatcherWithIndex(patterns: string[]): MatcherWithIndex {
  switch (patterns.length) {
    case 0: {
      return () => -1
    }

    case 1: {
      return matcherWhenOnlyOnePatternWithIndex(patterns[0])
    }
  }

  const matchArr: MatcherFunction[] = []

  let hasIgnore = false

  let hasInclude = false

  for (const pattern of patterns) {
    if (isIgnorePattern(pattern)) {
      hasIgnore = true

      matchArr.push({
        ignore: true,
        match: matcherFromPattern(pattern.substring(1)),
      })
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

function matchInputWithNonIgnoreMatchers(
  matchArr: MatcherFunction[],
  input: string
): number {
  for (let i = 0; i < matchArr.length; i++) {
    if (matchArr[i]?.match(input)) {
      return i
    }
  }

  return -1
}

function matchInputWithoutIgnoreMatchers(
  matchArr: MatcherFunction[],
  input: string
): number {
  return matchArr.some(({ match }) => match(input)) ? -1 : 0
}

function matchInputWithMatchersArray(
  matchArr: MatcherFunction[],
  input: string
): number {
  let matchedPatternIndex = -1

  matchArr.forEach((element: {
    match: Matcher;
    ignore: boolean;
  }, i: number): void => {
    const { ignore, match } = element

    if (ignore) {
      if (match(input)) {
        matchedPatternIndex = -1
      }
    } else if (matchedPatternIndex === -1 && match(input)) {
      matchedPatternIndex = i
    }
  });

  return matchedPatternIndex
}

function matcherFromPattern(pattern: string | undefined): Matcher {
  if (pattern === '*') {
    return () => true
  }

  const escapedPattern = escapeStringRegexp(pattern ?? '').replace(/\\\*/g, '.*')

  if (escapedPattern === pattern) {
    return (input: string) => input === pattern
  }

  const regexp = new RegExp(`^${escapedPattern}$`)

  return (input: string) => regexp.test(input)
}

function isIgnorePattern(pattern: string | undefined): boolean {
  return pattern?.startsWith('!') ?? false
}

function matcherWhenOnlyOnePatternWithIndex(pattern: string | undefined): MatcherWithIndex {
  const m = matcherWhenOnlyOnePattern(pattern)

  return (input) => (m(input) ? 0 : -1)
}

function matcherWhenOnlyOnePattern(pattern: string | undefined): Matcher {
  if (!isIgnorePattern(pattern)) {
    return matcherFromPattern(pattern)
  }

  const ignorePattern = pattern?.substring(1)

  const m = matcherFromPattern(ignorePattern)

  return (input: string): boolean => {
    return !m(input);
  }
}
