import getNodeMirror from '../lib/getNodeMirror'

test.each([
  ['release', { 'node-mirror:release': 'http://test.mirror.localhost/release' }, 'http://test.mirror.localhost/release/'],
  ['nightly', { 'node-mirror:nightly': 'http://test.mirror.localhost/nightly' }, 'http://test.mirror.localhost/nightly/'],
  ['rc', { 'node-mirror:rc': 'http://test.mirror.localhost/rc' }, 'http://test.mirror.localhost/rc/'],
  ['test', { 'node-mirror:test': 'http://test.mirror.localhost/test' }, 'http://test.mirror.localhost/test/'],
  ['v8-canary', { 'node-mirror:v8-canary': 'http://test.mirror.localhost/v8-canary' }, 'http://test.mirror.localhost/v8-canary/'],
])('getNodeMirror(%s, %s)', (releaseDir, rawConfig, expected) => {
  expect(getNodeMirror(rawConfig, releaseDir)).toBe(expected)
})

test('getNodeMirror uses defaults', () => {
  const rawConfig = {}
  expect(getNodeMirror(rawConfig, 'release')).toBe('https://nodejs.org/download/release/')
})

test('getNodeMirror returns base url with trailing /', () => {
  const rawConfig = {
    'node-mirror:release': 'http://test.mirror.localhost',
  }
  expect(getNodeMirror(rawConfig, 'release')).toBe('http://test.mirror.localhost/')
})
