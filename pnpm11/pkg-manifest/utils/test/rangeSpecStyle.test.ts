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
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '>=3.0.0-rc.8' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', { prevSpecifier: '2 || 3' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.0.0-rc.11', {})).toBe('3.0.0-rc.11')
})

test('calcVersionRange() ignores the requested specifier range style for a prerelease', () => {
  expect(calcVersionRange('3.0.0-rc.11', { bareSpecifier: '~3.0.0-rc.8' })).toBe('3.0.0-rc.11')
  expect(calcVersionRange('3.1.0', { bareSpecifier: '~3.0.0' })).toBe('~3.1.0')
})

test('rangeSpecGranularity() collapses exact to patch', () => {
  expect(rangeSpecGranularity('exact')).toBe('patch')
  expect(rangeSpecGranularity('patch')).toBe('patch')
  expect(rangeSpecGranularity('minor')).toBe('minor')
  expect(rangeSpecGranularity('major')).toBe('major')
  expect(rangeSpecGranularity('none')).toBe('none')
})
