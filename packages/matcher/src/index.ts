import escapeStringRegexp from 'escape-string-regexp'

type Matcher = (input: string) => boolean

export default function matcher (patterns: string[] | string): Matcher {
  if (typeof patterns === 'string') return matcherWhenOnlyOnePattern(patterns)
  switch (patterns.length) {
  case 0: return () => false
  case 1: return matcherWhenOnlyOnePattern(patterns[0])
  }
  const matchArr: Array<{ match: Matcher, negation: boolean }> = []
  for (const pattern of patterns) {
    if (isIgnorePattern(pattern)) {
      matchArr.push({ negation: true, match: matcherFromPattern(pattern.substring(1)) })
    } else {
      matchArr.push({ negation: false, match: matcherFromPattern(pattern) })
    }
  }
  return (input: string) => {
    let result = false
    for (const matcher of matchArr) {
      if (matcher.negation) {
        result = !matcher.match(input)
      } else if (matcher.match(input)) {
        result = true
      }
    }
    return result
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

function matcherWhenOnlyOnePattern (pattern: string): Matcher {
  return isIgnorePattern(pattern)
    ? () => false
    : matcherFromPattern(pattern)
}
