import { createHash } from 'node:crypto'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { rimrafSync } from '@zkochan/rimraf'

import { testDefaults } from '../utils/index.js'

afterEach(teardownMockAgent)

test('trustLockfile checks the release age of a replacement for an unavailable locked version', async () => {
  const project = prepareEmpty()
  const manifest = { dependencies: { 'trust-fallback-fixture': '^1.0.0' } }
  const seed = testDefaults({ lockfileOnly: true }, { retry: { retries: 0 } })
  const registry = seed.registriesByScope.default
  const { origin, pathname } = new URL(registry)
  const metadata = (version: string, published: string) => ({
    name: 'trust-fallback-fixture',
    'dist-tags': { latest: version },
    versions: {
      [version]: {
        name: 'trust-fallback-fixture',
        version,
        dist: {
          integrity: `sha512-${createHash('sha512').update(version).digest('base64')}`,
          tarball: `${registry}trust-fallback-fixture-${version}.tgz`,
        },
      },
    },
    time: { [version]: published },
  })
  await setupMockAgent()
  const pool = getMockAgent().get(origin)
  pool.intercept({ path: `${pathname}trust-fallback-fixture`, method: 'GET' })
    .reply(200, metadata('1.0.0', '2020-01-01T00:00:00Z'))
  await install(manifest, seed)
  expect(project.readLockfile().snapshots).toHaveProperty(['trust-fallback-fixture@1.0.0'])
  rimrafSync(seed.cacheDir)
  pool.intercept({ path: `${pathname}trust-fallback-fixture`, method: 'GET' })
    .reply(200, metadata('1.0.1', new Date().toISOString())).persist()
  const violations: string[] = []
  await install(manifest, {
    ...testDefaults({
      storeDir: path.resolve('fresh-store'),
      lockfileOnly: true,
      forceFullResolution: true,
      preferFrozenLockfile: false,
      trustLockfile: true,
      minimumReleaseAge: 1440,
      minimumReleaseAgeStrict: false,
    }, { retry: { retries: 0 } }),
    handleResolutionPolicyViolations: async (found) => {
      violations.push(...found.map(({ name, version }) => `${name}@${version}`))
    },
  })
  expect(violations).toStrictEqual(['trust-fallback-fixture@1.0.1'])
  expect(project.readLockfile().snapshots).toHaveProperty(['trust-fallback-fixture@1.0.1'])
})
