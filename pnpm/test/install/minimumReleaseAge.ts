import path from 'node:path'

import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

const LODASH_4_17_21_PUBLISHED = new Date('2024-06-03T22:09:35.290Z').getTime()
const LODASH_4_17_23_PUBLISHED = new Date('2026-01-21T17:29:52.831Z').getTime()
const LODASH_4_18_0_PUBLISHED = new Date('2026-03-31T18:18:42.717Z').getTime()
const LODASH_4_18_1_PUBLISHED = new Date('2026-04-01T21:01:20.458Z').getTime()
const MINUTE_MS = 60 * 1000

// This intentionally uses the public npm registry, similar to the minimumReleaseAge
// dlx coverage in pnpm/test/dlx.ts. The current regression reproduces against
// the live lodash packument but did not reproduce with a synthetic local packument.
// This is a repro-only test and may need to be updated if the lodash release graph changes.
test('install should fall back to the newest mature same-major version when latest is too young', () => {
  const project = prepare({
    dependencies: {
      lodash: '^4.17.21',
    },
  })
  // Expected maturity state for the range ^4.17.21:
  // - 4.17.21: mature
  // - 4.17.23: mature
  // - 4.18.0: mature and should be selected
  // - 4.18.1: too young and should be ignored
  const minimumReleaseAge = Math.floor((Date.now() - LODASH_4_18_0_PUBLISHED) / MINUTE_MS) - 60
  const cutoff = Date.now() - minimumReleaseAge * MINUTE_MS

  expect(LODASH_4_17_21_PUBLISHED).toBeLessThan(cutoff)
  expect(LODASH_4_17_23_PUBLISHED).toBeLessThan(cutoff)
  expect(LODASH_4_18_0_PUBLISHED).toBeLessThan(cutoff)
  expect(LODASH_4_18_1_PUBLISHED).toBeGreaterThan(cutoff)

  writeYamlFileSync('pnpm-workspace.yaml', {
    minimumReleaseAge,
  })

  execPnpmSync([
    `--config.cache-dir=${path.resolve('cache')}`,
    `--config.store-dir=${path.resolve('store')}`,
    '--config.registry=https://registry.npmjs.org/',
    'install',
    '--lockfile-only',
  ], {
    expectSuccess: true,
    omitEnvDefaults: ['pnpm_config_minimum_release_age'],
    timeout: 120000,
  })

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.lodash?.version).toBe('4.18.0')
})
