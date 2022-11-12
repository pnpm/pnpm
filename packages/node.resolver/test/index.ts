import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersionList } from '@pnpm/node.resolver'
import semver from 'semver'

const fetch = createFetchFromRegistry({})

test('resolve Node.js version list', async () => {
  const versions = await resolveNodeVersionList(fetch, '16')
  expect(versions.every(version => semver.satisfies(version, '16'))).toBe(true)
})
