import escapeStringRegexp = require('escape-string-regexp')

export default function matcher (patterns: string[] | string) {
  if (typeof patterns === 'string') return matcherFromPattern(patterns)
  switch (patterns.length) {
  case 0: return () => false
  case 1: return matcherFromPattern(patterns[0])
  }
  const matchArr = patterns.map(matcherFromPattern)
  return (input: string) => matchArr.some((match) => match(input))
}

function matcherFromPattern (pattern: string) {
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
