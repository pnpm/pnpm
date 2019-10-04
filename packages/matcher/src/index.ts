import escapeStringRegexp = require('escape-string-regexp')

export default function matcher (pattern: string) {
  const regexp = makeRegexp(pattern)
  return match.bind(match, regexp)
}

function makeRegexp (pattern: string) {
  pattern = escapeStringRegexp(pattern).replace(/\\\*/g, '.*').replace(/\\\|/g, '|')

  const regexp = new RegExp(`^${pattern}$`)
  return regexp
}

function match (regexp: RegExp, input: string) {
  return regexp.test(input)
}
