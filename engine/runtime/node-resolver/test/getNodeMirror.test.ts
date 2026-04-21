import { expect, test } from '@jest/globals'

import { getNodeMirror } from '../lib/getNodeMirror.js'

test.each([
  ['release', { release: 'http://test.mirror.localhost/release' }, 'http://test.mirror.localhost/release/'],
  ['nightly', { nightly: 'http://test.mirror.localhost/nightly' }, 'http://test.mirror.localhost/nightly/'],
  ['rc', { rc: 'http://test.mirror.localhost/rc' }, 'http://test.mirror.localhost/rc/'],
  ['test', { test: 'http://test.mirror.localhost/test' }, 'http://test.mirror.localhost/test/'],
  ['v8-canary', { 'v8-canary': 'http://test.mirror.localhost/v8-canary' }, 'http://test.mirror.localhost/v8-canary/'],
])('getNodeMirror(%s, %s)', (releaseDir, mirrors, expected) => {
  expect(getNodeMirror(mirrors, releaseDir)).toBe(expected)
})

test('getNodeMirror uses defaults', () => {
  expect(getNodeMirror({}, 'release')).toBe('https://nodejs.org/download/release/')
})

test('getNodeMirror with undefined mirrors uses defaults', () => {
  expect(getNodeMirror(undefined, 'release')).toBe('https://nodejs.org/download/release/')
})

test('getNodeMirror returns base url with trailing /', () => {
  expect(getNodeMirror({ release: 'http://test.mirror.localhost' }, 'release')).toBe('http://test.mirror.localhost/')
})
