import { describe, expect, test } from '@jest/globals'

import { isSubRange } from '../src/isSubRange.js'

describe('isSubRange()', () => {
  test('absent or empty broader range covers every narrower range', () => {
    expect(isSubRange(undefined, '^1.0.0')).toBe(true)
    expect(isSubRange('', '^1.0.0')).toBe(true)
    expect(isSubRange('^1.0.0', undefined)).toBe(false)
    expect(isSubRange('^1.0.0', '')).toBe(false)
    expect(isSubRange(undefined, undefined)).toBe(true)
  })

  test('exact and tilde ranges are subsets of enclosing caret range', () => {
    expect(isSubRange('^1.0.0', '1.0.0')).toBe(true)
    expect(isSubRange('1.0.0', '^1.0.0')).toBe(false)
    expect(isSubRange('^1.0.0', '~1.2.0')).toBe(true)
    expect(isSubRange('~1.2.0', '^1.0.0')).toBe(false)
  })

  test('adjacent union branches combine coverage', () => {
    expect(isSubRange('^1.0.0 || ^2.0.0', '>=1.5.0 <2.5.0')).toBe(true)
    expect(isSubRange('>=1.5.0 <2.5.0', '^1.0.0 || ^2.0.0')).toBe(false)
    expect(isSubRange('>=1.0.0 <2.0.0 || >=2.0.0 <3.0.0', '>=1.5.0 <2.5.0')).toBe(true)
    expect(isSubRange('1.0.0 || 1.0.1', '>=1.0.0 <=1.0.1')).toBe(true)
    expect(isSubRange('>=1.0.0 <=1.0.1', '1.0.0 || 1.0.1')).toBe(true)
    expect(
      isSubRange('>=1.0.0-alpha.0 <1.0.0-alpha.5 || >=1.0.0-alpha.5 <1.0.0', '>=1.0.0-alpha.2 <1.0.0')
    ).toBe(true)
    expect(isSubRange('^1.0.0 || ^3.0.0', '>=1.5.0 <2.5.0')).toBe(false)
  })

  test('upper-only, wildcard, prerelease, and empty-intersection ranges match npm semver rules', () => {
    expect(isSubRange('*', '<1.0.0')).toBe(true)
    expect(isSubRange('*', '<0.0.0-alpha.5')).toBe(false)
    expect(isSubRange('*', '1.0.0-alpha.1')).toBe(false)
    expect(isSubRange('*', '* || 1.0.0-alpha.1')).toBe(true)
    expect(isSubRange('* || 1.0.0-alpha.1', '*')).toBe(true)
    expect(isSubRange('^1.0.0-alpha.0', '1.0.0-alpha.1 || 1.0.0')).toBe(true)
    expect(isSubRange('>=1.0.0-alpha.1 <=2.0.0-beta.1', '1.0.0-alpha.2 || 2.0.0-beta.0')).toBe(true)
    expect(isSubRange('>=1.0.0-alpha.1 <=2.0.0-beta.1', '1.5.0-alpha.1')).toBe(false)
    expect(isSubRange('>=1.0.0', '>=1.0.0-alpha.1 >=1.0.0')).toBe(true)
    expect(isSubRange('^1.0.0', '>=1.0.0-alpha.1 >=1.0.0 <2.0.0-0')).toBe(true)
    expect(isSubRange('^1.0.0 || ^2.0.0 || ^3.0.0', '1.0.0 || 2.0.0 || 3.0.0')).toBe(true)
    expect(isSubRange('^1.0.0 || ^3.0.0', '1.0.0 || 2.0.0 || 3.0.0')).toBe(false)
    expect(isSubRange('^1.0.0', '>2.0.0 <1.0.0')).toBe(true)
    expect(isSubRange('^1.0.0', '<0.0.0')).toBe(true)
    expect(isSubRange('>2.0.0 <1.0.0', '^1.0.0')).toBe(false)
  })

  test('large numeric prerelease identifiers compare numerically like node-semver', () => {
    expect(isSubRange('>=1.0.0-9007199254740992', '1.0.0-10000000000000000')).toBe(true)
    expect(isSubRange('1.0.0-10000000000000000', '>=1.0.0-9007199254740992')).toBe(false)
    expect(
      isSubRange('>=1.0.0-9007199254740992', '>=1.0.0-9007199254740992 <1.0.0-10000000000000000')
    ).toBe(true)
    expect(
      isSubRange('>=1.0.0-9007199254740992 <1.0.0-10000000000000000', '>=1.0.0-9007199254740992')
    ).toBe(false)
  })

  test('non-semver specifiers only cover identical non-semver specifiers', () => {
    expect(isSubRange('*', 'latest')).toBe(false)
    expect(isSubRange('latest', '*')).toBe(false)
    expect(isSubRange('workspace:*', 'workspace:*')).toBe(true)
    expect(isSubRange('workspace:*', 'workspace:^')).toBe(false)
  })
})
