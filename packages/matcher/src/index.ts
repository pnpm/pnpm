import escapeStringRegexp = require('escape-string-regexp')

export default function matcher (patterns: string[] | string) {
  if (typeof patterns === 'string') return matcherFromPattern(patterns)
  if (patterns.length === 0) return matcherFromPattern(patterns[0])
  const matchArr = patterns.map(matcherFromPattern)
  return (input: string) => matchArr.some((match) => match(input))
}

function matcherFromPattern (pattern: string) {
  const regexp = makeRegexp(pattern)
  return match.bind(match, regexp)
}

function makeRegexp (pattern: string) {
  pattern = escapeStringRegexp(pattern).replace(/\\\*/g, '.*')

  const regexp = new RegExp(`^${pattern}$`)
  return regexp
}

function match (regexp: RegExp, input: string) {
  return regexp.test(input)
}
