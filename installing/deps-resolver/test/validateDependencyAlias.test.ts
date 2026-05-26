import { expect, test } from '@jest/globals'

import { assertValidDependencyAliases, isValidDependencyAlias } from '../lib/validateDependencyAlias.js'

test.each([
  ['foo'],
  ['Foo'],
  ['@scope/name'],
  ['@s/x'],
  ['lodash.merge'],
  ['a_b'],
  ['a-b'],
  ['x'],
  ['underscore'],
])('accepts %p', (alias) => {
  expect(isValidDependencyAlias(alias)).toBe(true)
})

test.each([
  ['', 'empty string'],
  ['..', 'parent traversal'],
  ['.', 'current dir'],
  ['/foo', 'absolute posix'],
  ['foo/bar', 'unscoped slash'],
  ['@scope/name/extra', 'scoped with extra segment'],
  ['@scope/../etc', 'scope with parent traversal'],
  ['@x/../../../../../.git/hooks', 'PoC payload'],
  ['foo\\bar', 'backslash'],
  ['C:\\Windows\\System32', 'windows absolute'],
  ['foo\0bar', 'null byte'],
  ['scope/name', 'two segments without @'],
  ['./foo', 'current dir prefix'],
  ['.bin', 'leading dot (collides with pnpm .bin)'],
  ['.pnpm', 'leading dot (collides with pnpm .pnpm)'],
  ['_foo', 'leading underscore'],
  ['node_modules', 'reserved name'],
  ['favicon.ico', 'reserved name'],
  ['  foo  ', 'leading/trailing whitespace'],
  ['foo bar', 'embedded whitespace'],
  ['foo?bar', 'non-url-friendly character'],
])('rejects %s (%s)', (alias) => {
  expect(isValidDependencyAlias(alias)).toBe(false)
})

test('assertValidDependencyAliases throws ERR_PNPM_INVALID_DEPENDENCY_NAME for malicious aliases', () => {
  expect(() => {
    assertValidDependencyAliases({ '@x/../../../../../.git/hooks': '1.0.0' }, 'Package "bad@1.0.0"')
  }).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
    message: expect.stringContaining('Package "bad@1.0.0" contains a dependency with an invalid name'),
  }))
})

test('assertValidDependencyAliases is a no-op for undefined and empty input', () => {
  expect(() => {
    assertValidDependencyAliases(undefined, 'pkg')
  }).not.toThrow()
  expect(() => {
    assertValidDependencyAliases({}, 'pkg')
  }).not.toThrow()
})

test('assertValidDependencyAliases is a no-op for valid aliases', () => {
  expect(() => {
    assertValidDependencyAliases({ foo: '1.0.0', '@scope/bar': '2.0.0' }, 'pkg')
  }).not.toThrow()
})
