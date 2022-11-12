import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersions } from '@pnpm/node.resolver'

const fetch = createFetchFromRegistry({})

test.each([
  ['https://nodejs.org/download/release/', '6', '6.17.1'],
  ['https://nodejs.org/download/rc/', '16.0.0-rc.0', '16.0.0-rc.0'],
  ['https://nodejs.org/download/rc/', '10', '10.23.0-rc.0'],
  ['https://nodejs.org/download/nightly/', 'latest', /.+/],
  ['https://nodejs.org/download/release/', 'lts', /.+/],
  ['https://nodejs.org/download/release/', 'argon', '4.9.1'],
  ['https://nodejs.org/download/release/', 'latest', /.+/],
  [undefined, 'latest', /.+/],
])('Node.js %s is resolved', async (nodeMirrorBaseUrl, spec, expectedVersion) => {
  const versions = await resolveNodeVersions(fetch, {
    versionSpec: spec,
    nodeMirrorBaseUrl,
    useHighest: true,
  })
  expect(versions.length).toEqual(1)
  expect(versions[0]).toMatch(expectedVersion)
})

test('resolve specified version list', async () => {
  const versions = await resolveNodeVersions(fetch, {
    versionSpec: '16',
  })
  expect(versions.length).toBeGreaterThan(1)
  expect(versions.every(version => version.match(/^16.+/))).toBeTruthy()
})
