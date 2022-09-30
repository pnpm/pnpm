import escapeStringRegexp from 'escape-string-regexp'

type Matcher = (input: string) => boolean
type MatcherWithIndex = (input: string) => number

export default function matcher (patterns: string[] | string): Matcher {
  const m = matcherWithIndex(Array.isArray(patterns) ? patterns : [patterns])
  return (input) => m(input) !== -1
}

export function matcherWithIndex (patterns: string[]): MatcherWithIndex {
  switch (patterns.length) {
  case 0: return () => -1
  case 1: return matcherWhenOnlyOnePatternWithIndex(patterns[0])
  }
  const matchArr: Array<{ match: Matcher, ignore: boolean }> = []
  let hasIgnore = false
  for (const pattern of patterns) {
    if (isIgnorePattern(pattern)) {
      hasIgnore = true
      matchArr.push({ ignore: true, match: matcherFromPattern(pattern.substring(1)) })
    } else {
      matchArr.push({ ignore: false, match: matcherFromPattern(pattern) })
    }
  }
  if (!hasIgnore) {
    return (input: string) => {
      for (let i = 0; i < matchArr.length; i++) {
        if (matchArr[i].match(input)) return i
      }
      return -1
    }
  }
  return (input: string) => {
    let isMatched = -1
    for (let i = 0; i < matchArr.length; i++) {
      const { ignore, match } = matchArr[i]
      if (ignore) {
        if (!match(input)) {
          isMatched = isMatched === -1 ? i : isMatched
        } else {
          isMatched = -1
        }
      } else if (isMatched === -1 && match(input)) {
        isMatched = i
      }
    }
    return isMatched
  }
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
  return pattern.startsWith('!')
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
