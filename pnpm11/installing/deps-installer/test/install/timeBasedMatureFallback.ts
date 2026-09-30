import { afterEach, expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'

import { testDefaults } from '../utils/index.js'

afterEach(teardownMockAgent)

test.each(['highest', 'time-based'] as const)('time-based fallback chooses a mature version under %s', async (resolutionMode) => {
  const project = prepareEmpty()
  const violations: string[] = []
  const opts = testDefaults({
    minimumReleaseAge: (Date.now() - Date.parse('2022-05-15T00:00:00Z')) / 60000,
    minimumReleaseAgeStrict: true,
    resolutionMode,
    overrides: { 'fallback-child': '^1.0.2' },
    lockfileOnly: true,
  }, { retry: { retries: 0 } })
  await setupMockAgent()
  const registry = opts.registriesByScope.default.replace(/\/$/, '')
  const pool = getMockAgent().get(registry)
  const manifest = (name: string, version: string, dependencies = {}) => ({
    name, version, dependencies,
    dist: { integrity: 'sha512-' + Buffer.alloc(64).toString('base64'), tarball: `${registry}/${name}-${version}.tgz` },
  })
  pool.intercept({ path: '/fallback-parent', method: 'GET' }).reply(200, {
    name: 'fallback-parent',
    'dist-tags': { latest: '1.0.0' },
    versions: { '1.0.0': manifest('fallback-parent', '1.0.0', { 'fallback-child': '^1.0.0' }) },
    time: { '1.0.0': '2022-04-01T00:00:00Z' },
  }).persist()
  pool.intercept({ path: '/fallback-child', method: 'GET' }).reply(200, {
    name: 'fallback-child',
    'dist-tags': { latest: '1.1.0' },
    versions: {
      '1.0.0': manifest('fallback-child', '1.0.0'),
      '1.0.2': manifest('fallback-child', '1.0.2'),
      '1.1.0': manifest('fallback-child', '1.1.0'),
    },
    time: { '1.0.0': '2022-02-01T00:00:00Z', '1.0.2': '2022-06-01T00:00:00Z', '1.1.0': '2022-05-01T00:00:00Z' },
  }).persist()
  await install({ dependencies: { 'fallback-parent': '1.0.0' } }, {
    ...opts,
    handleResolutionPolicyViolations: async (found) => {
      violations.push(...found.map((violation) => `${violation.name}@${violation.version} ${violation.reason}`))
    },
  })
  expect(violations).toStrictEqual([])
  expect(project.readLockfile().snapshots).toHaveProperty(['fallback-child@1.1.0'])
})
