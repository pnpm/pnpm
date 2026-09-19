import { expect, test } from '@jest/globals'
import { sanitizeInline } from '@pnpm/text.sanitize'

test('sanitizeInline strips escape sequences', () => {
  expect(sanitizeInline('foo\u001B[2K\u001B[G')).toBe('foo[2K[G')
})

test('sanitizeInline strips invisible formatting characters', () => {
  expect(sanitizeInline('foo\u202Ebar')).toBe('foobar')
  expect(sanitizeInline('foo\u200Bbar\uFEFF')).toBe('foobar')
})

test('sanitizeInline strips line breaks and tabs', () => {
  expect(sanitizeInline('a\nb\tc')).toBe('abc')
})

test('sanitizeInline leaves text without control characters untouched', () => {
  expect(sanitizeInline('@scope/foo@1.0.0')).toBe('@scope/foo@1.0.0')
})

test('sanitizeInline() strips Unicode line and paragraph separators', () => {
  // U+2028 and U+2029 end a line for a JSON or JavaScript consumer without
  // being control characters, so a name carrying one could split a rendered
  // warning into a second line.
  expect(sanitizeInline('foo-\u2028\u2029-bar')).toBe('foo--bar')
})
