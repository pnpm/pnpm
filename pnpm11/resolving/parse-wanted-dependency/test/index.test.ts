import { describe, expect, test } from '@jest/globals'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'

describe('parseWantedDependency', () => {
  test('returns empty bareSpecifier when called with no arguments or undefined', () => {
    expect(parseWantedDependency()).toStrictEqual({ bareSpecifier: '' })
    expect(parseWantedDependency(undefined)).toStrictEqual({ bareSpecifier: '' })
  })

  test('returns empty bareSpecifier when called with empty string', () => {
    expect(parseWantedDependency('')).toStrictEqual({ bareSpecifier: '' })
  })

  test('parses plain package name', () => {
    expect(parseWantedDependency('foo')).toStrictEqual({ alias: 'foo' })
  })

  test('parses scoped package name', () => {
    expect(parseWantedDependency('@scope/foo')).toStrictEqual({ alias: '@scope/foo' })
  })

  test('parses plain package name with version', () => {
    expect(parseWantedDependency('foo@1.0.0')).toStrictEqual({
      alias: 'foo',
      bareSpecifier: '1.0.0',
    })
  })

  test('parses scoped package name with version', () => {
    expect(parseWantedDependency('@scope/foo@1.0.0')).toStrictEqual({
      alias: '@scope/foo',
      bareSpecifier: '1.0.0',
    })
  })

  test('parses package name with tag', () => {
    expect(parseWantedDependency('foo@latest')).toStrictEqual({
      alias: 'foo',
      bareSpecifier: 'latest',
    })
  })

  test('keeps bare url specifier', () => {
    expect(parseWantedDependency('https://example.com/foo.tgz')).toStrictEqual({
      bareSpecifier: 'https://example.com/foo.tgz',
    })
  })
})
