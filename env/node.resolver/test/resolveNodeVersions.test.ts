import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersions } from '@pnpm/node.resolver'

const fetch = createFetchFromRegistry({})

test('resolve specified version list', async () => {
  const versions = await resolveNodeVersions(fetch, '16')
  expect(versions.length).toBeGreaterThan(1)
  expect(versions.every(version => version.match(/^16.+/))).toBeTruthy()
})

test('resolve latest version', async () => {
  const versions = await resolveNodeVersions(fetch, 'latest')
  expect(versions.length).toEqual(1)
})

test('resolve all versions', async () => {
  const versions = await resolveNodeVersions(fetch)
  expect(versions.length).toBeGreaterThan(1)
})
