import { expect, test } from '@jest/globals'
import { resolveNodeVersion } from '@pnpm/engine.runtime.node-resolver'
import { createFetchFromRegistry } from '@pnpm/network.fetch'

const fetch = createFetchFromRegistry({})

test.each([
  ['https://nodejs.org/download/release/', '6', '6.17.1'],
  ['https://nodejs.org/download/rc/', '16.0.0-rc.0', '16.0.0-rc.0'],
  ['https://nodejs.org/download/rc/', '10', '10.23.0-rc.0'],
  ['https://nodejs.org/download/release/', 'lts', /.+/],
  ['https://nodejs.org/download/release/', 'argon', '4.9.1'],
  ['https://nodejs.org/download/release/', 'latest', /.+/],
  ['https://nodejs.org/download/release/', '', /.+/],
  ['https://nodejs.org/download/release/', '  ', /.+/],
  [undefined, 'latest', /.+/],
])('Node.js %s is resolved', async (nodeMirrorBaseUrl, spec, expectedVersion) => {
  const version = await resolveNodeVersion(fetch, spec, nodeMirrorBaseUrl)
  expect(version).toMatch(expectedVersion)
})

test('latest selects the newest stable version when the index starts with a prerelease', async () => {
  const mirror = 'https://node.example/download/release/'
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-alpha.0.0.0', lts: false },
    { version: 'v26.13.0', lts: false },
    { version: 'v26.12.0-rc.1', lts: false },
  ]))

  await expect(resolveNodeVersion(mockFetch, 'latest', mirror)).resolves.toBe('26.13.0')
  await expect(resolveNodeVersion(mockFetch, '', mirror)).resolves.toBe('26.13.0')
})

test('latest has no match when the index contains only prereleases', async () => {
  const mirror = 'https://node.example/download/release/'
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-alpha.0.0.0', lts: false },
  ]))

  await expect(resolveNodeVersion(mockFetch, 'latest', mirror)).resolves.toBeNull()
})

test('latest follows the selected prerelease channel', async () => {
  const mirror = 'https://nodejs.org/download/nightly/'
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-nightly20261029abcdef', lts: false },
  ]))

  await expect(resolveNodeVersion(mockFetch, 'latest', { nodeMirrorBaseUrl: mirror, releaseChannel: 'nightly' }))
    .resolves.toBe('27.0.0-nightly20261029abcdef')
})
