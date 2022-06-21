import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersion } from '@pnpm/node.resolver'

const fetch = createFetchFromRegistry({})

test.each([
  ['https://nodejs.org/download/release/', '6', '6.17.1'],
  ['https://nodejs.org/download/rc/', '16.0.0-rc.0', '16.0.0-rc.0'],
  ['https://nodejs.org/download/rc/', '10', '10.23.0-rc.0'],
  ['https://nodejs.org/download/nightly/', 'latest', /.+/],
  ['https://nodejs.org/download/release/', 'lts', /.+/],
  ['https://nodejs.org/download/release/', 'argon', '4.9.1'],
  ['https://nodejs.org/download/release/', 'latest', /.+/],
])('Node.js %s is resolved', async (nodeMirrorBaseUrl, spec, expectedVersion) => {
  const version = await resolveNodeVersion(fetch, spec, nodeMirrorBaseUrl)
  expect(version).toMatch(expectedVersion)
})
