import escapeStringRegexp from 'escape-string-regexp'

type Matcher = (input: string) => boolean

export default function matcher (patterns: string[] | string): Matcher {
  if (typeof patterns === 'string') return matcherWhenOnlyOnePattern(patterns)
  switch (patterns.length) {
  case 0: return () => false
  case 1: return matcherWhenOnlyOnePattern(patterns[0])
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
    return (input: string) => matchArr.some(({ match }) => match(input))
  }
  return (input: string) => {
    let isMatched = false
    for (const { ignore, match } of matchArr) {
      if (ignore) {
        isMatched = !match(input)
      } else if (!isMatched && match(input)) {
        isMatched = true
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

function matcherWhenOnlyOnePattern (pattern: string): Matcher {
  return isIgnorePattern(pattern)
    ? () => false
    : matcherFromPattern(pattern)
}
