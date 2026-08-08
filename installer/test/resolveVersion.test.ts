import { expect, test } from '@jest/globals'
import { type Packument, resolveVersion } from 'get-pnpm'

const packument: Packument = {
  'dist-tags': {
    latest: '11.20.0',
    'latest-10': '10.34.5',
    'latest-11': '11.20.0',
    'next-12': '12.0.0-rc.1',
  },
  versions: {
    '10.34.5': {},
    '11.19.0': {},
    '11.20.0': {},
    '12.0.0-rc.1': {},
  },
}

test('resolves a dist-tag', () => {
  expect(resolveVersion(packument, 'latest')).toBe('11.20.0')
  expect(resolveVersion(packument, 'next-12')).toBe('12.0.0-rc.1')
})

test('resolves an exact version, with or without a leading v', () => {
  expect(resolveVersion(packument, '11.19.0')).toBe('11.19.0')
  expect(resolveVersion(packument, 'v11.19.0')).toBe('11.19.0')
})

test('resolves a bare major to its stable release, with or without a leading v', () => {
  expect(resolveVersion(packument, '10')).toBe('10.34.5')
  expect(resolveVersion(packument, 'v10')).toBe('10.34.5')
})

test('falls back to the prerelease lane for a major with no stable release', () => {
  expect(resolveVersion(packument, '12')).toBe('12.0.0-rc.1')
})

test('lists the available tags when nothing matches', () => {
  expect(() => resolveVersion(packument, '9')).toThrow(/could not be found.*latest, latest-10, latest-11, next-12/)
  expect(() => resolveVersion(packument, '11.18.0')).toThrow(/version "11.18.0" could not be found/)
})
