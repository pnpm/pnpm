import { expect, test } from '@jest/globals'
import { calcVersionRange, getRangeSpecStyle, rangeSpecGranularity, versionWithRangeSpecStyle } from '@pnpm/pkg-manifest.utils'

test('getRangeSpecStyle()', () => {
  expect(getRangeSpecStyle({ saveExact: true })).toBe('patch')
  expect(getRangeSpecStyle({ savePrefix: '' })).toBe('patch')
  expect(getRangeSpecStyle({ savePrefix: '=' })).toBe('exact')
  expect(getRangeSpecStyle({ savePrefix: '~' })).toBe('minor')
  expect(getRangeSpecStyle({ savePrefix: '^' })).toBe('major')
  expect(getRangeSpecStyle({})).toBe('major')
  expect(getRangeSpecStyle({ saveExact: true, savePrefix: '=' })).toBe('patch')
})

test('versionWithRangeSpecStyle()', () => {
  expect(versionWithRangeSpecStyle('1.2.3', 'major')).toBe('^1.2.3')
  expect(versionWithRangeSpecStyle('1.2.3', 'minor')).toBe('~1.2.3')
  expect(versionWithRangeSpecStyle('1.2.3', 'patch')).toBe('1.2.3')
  expect(versionWithRangeSpecStyle('1.2.3', 'exact')).toBe('=1.2.3')
  expect(versionWithRangeSpecStyle('1.2.3', 'none')).toBe('^1.2.3')
  expect(() => versionWithRangeSpecStyle('1.2.3', 'bogus' as never)).toThrow("Unknown range spec style: 'bogus'")
})

test('calcVersionRange() preserves an existing prerelease range style', () => {
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '^3.0.0-rc.8' })).toBe('^3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '~3.0.0-rc.8' })).toBe('~3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '3.0.0-rc.8' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '=3.0.0-rc.8' })).toBe('=3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '>=3.0.0-rc.8' })).toBe('>=3.0.0-rc.8')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '2 || 3' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', {})).toBe('3.0.0-rc.11')
})

test('calcVersionRange() ignores the requested specifier range style for a prerelease', () => {
  expect(calcVersionRange('3.0.0-rc.11', { bareSpecifier: '~3.0.0-rc.8' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.1.0', { bareSpecifier: '~3.0.0' })).toBe('~3.1.0')
})

test.each([
  ['<= 1.2.5', '1.2.0', '<= 1.2.5'],
  ['<= 1.2.5', '100.1.0', '^100.1.0'],
  ['>=1.0.0 <1.3.0', '1.2.0', '>=1.0.0 <1.3.0'],
  ['>=1.0.0 <1.3.0', '100.1.0', '^100.1.0'],
  ['~5 <5.4.54 || ~5.4.55', '5.4.53', '~5 <5.4.54 || ~5.4.55'],
  ['=1.1.0', '1.1.0', '=1.1.0'],
  ['=1.1.0', '100.1.0', '=100.1.0'],
  ['~1.1.0', '1.1.0', '~1.1.0'],
  ['~1.1.0', '100.1.0', '~100.1.0'],
  ['^3.0.0-rc.0', '3.0.0-rc.1', '^3.0.0-rc.1'],
  ['1.1.0', '1.1.0', '1.1.0'],
  ['1.1.0', '100.1.0', '100.1.0'],
  ['*', '100.1.0', '^100.1.0'],
  ['npm:bar@<= 1.2.5', '1.2.0', '<= 1.2.5'],
  ['latest', '100.1.0', '^100.1.0'],
])('calcVersionRange() keeps the shape of the existing range %s when updating to %s', (prevSpecifier, version, expected) => {
  expect(calcVersionRange(version, { prevSpecifier, bareSpecifier: prevSpecifier })).toBe(expected)
})

test('calcVersionRange() lets a request that names a specifier replace a kept range', () => {
  expect(calcVersionRange('1.2.0', { prevSpecifier: '<= 1.2.5', bareSpecifier: '1.2.0' })).toBe('1.2.0')
  expect(calcVersionRange('1.2.0', { prevSpecifier: '<= 1.2.5', bareSpecifier: '^1.2.0' })).toBe('^1.2.0')
  expect(calcVersionRange('1.2.0', { prevSpecifier: '<= 1.2.5', bareSpecifier: '>=1.1.0 <1.3.0' })).toBe('^1.2.0')
  expect(calcVersionRange('1.2.0', { prevSpecifier: '<= 1.2.5', bareSpecifier: 'latest' })).toBe('^1.2.0')
})

test('rangeSpecGranularity() collapses exact to patch', () => {
  expect(rangeSpecGranularity('exact')).toBe('patch')
  expect(rangeSpecGranularity('patch')).toBe('patch')
  expect(rangeSpecGranularity('minor')).toBe('minor')
  expect(rangeSpecGranularity('major')).toBe('major')
  expect(rangeSpecGranularity('none')).toBe('none')
})
