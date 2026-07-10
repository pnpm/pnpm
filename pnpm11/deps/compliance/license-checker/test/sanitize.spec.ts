import { describe, expect, test } from '@jest/globals'

import { sanitizeForTerminal } from '../src/sanitize.js'

describe('sanitizeForTerminal', () => {
  test('strips ANSI escape sequences', () => {
    expect(sanitizeForTerminal('\x1b[2J\x1b]0;pwned\x07MIT')).toBe('MIT')
  })
  test('strips C0 control chars but keeps tab', () => {
    expect(sanitizeForTerminal('a\x07b\tc')).toBe('ab\tc')
  })
  test('passes clean strings through', () => {
    expect(sanitizeForTerminal('Apache-2.0 WITH LLVM-exception')).toBe('Apache-2.0 WITH LLVM-exception')
  })
})
