import escapeStringRegexp from 'escape-string-regexp'

type Matcher = (input: string) => boolean

export default function matcher (patterns: string[] | string): Matcher {
  if (typeof patterns === 'string') return matcherWhenOnlyOnePattern(patterns)
  switch (patterns.length) {
  case 0: return () => false
  case 1: return matcherWhenOnlyOnePattern(patterns[0])
  }
  return (input: string) => patterns.reduce((result, pattern) => {
    if (isIgnorePattern(pattern)) {
      const match = matcherFromPattern(pattern.substring(1))
      if (match(input)) {
        return false
      }
    } else {
      const match = matcherFromPattern(pattern)
      if (match(input)) {
        return true
      }
    }

    return result
  }, false)
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
