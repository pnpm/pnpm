import { expect, test } from '@jest/globals'
import { resolveNodeVersions } from '@pnpm/engine.runtime.node-resolver'
import { createFetchFromRegistry } from '@pnpm/network.fetch'

const fetch = createFetchFromRegistry({})

test('resolve specified version list', async () => {
  const versions = await resolveNodeVersions(fetch, '16')
  expect(versions.length).toBeGreaterThan(1)
  expect(versions.every(version => version.match(/^16.+/))).toBeTruthy()
})

test('resolve latest version', async () => {
  const versions = await resolveNodeVersions(fetch, 'latest')
  expect(versions).toHaveLength(1)
})

test.each(['', '  '])('resolve blank version list as latest (%j)', async (spec) => {
  const versions = await resolveNodeVersions(fetch, spec)
  expect(versions).toHaveLength(1)
})

test('resolve all versions', async () => {
  const versions = await resolveNodeVersions(fetch)
  expect(versions.length).toBeGreaterThan(1)
})

test('resolve latest skips prereleases at the start of the index', async () => {
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-alpha.0.0.0', lts: false },
    { version: 'v26.13.0', lts: false },
  ]))

  await expect(resolveNodeVersions(mockFetch, 'latest', 'https://node.example/')).resolves.toEqual(['26.13.0'])
})

test('resolve latest returns no versions when the index has only prereleases', async () => {
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-alpha.0.0.0', lts: false },
  ]))

  await expect(resolveNodeVersions(mockFetch, 'latest', 'https://node.example/')).resolves.toEqual([])
})

test('resolve latest follows the selected prerelease channel', async () => {
  const mockFetch = async () => new Response(JSON.stringify([
    { version: 'v27.0.0-nightly20261029abcdef', lts: false },
  ]))

  await expect(resolveNodeVersions(mockFetch, 'latest', {
    nodeMirrorBaseUrl: 'https://node.example/nightly/',
    releaseChannel: 'nightly',
  })).resolves.toEqual(['27.0.0-nightly20261029abcdef'])
})
