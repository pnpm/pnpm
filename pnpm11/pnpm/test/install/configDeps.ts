import { expect, test } from '@jest/globals'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

test('installing configDependencies migrating to env lockfile', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/foo': '100.0.0+' + getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })

  await execPnpm(['install'])

  const envLockfile = await readEnvLockfile(process.cwd())
  expect(envLockfile?.importers['.'].configDependencies['@pnpm.e2e/foo'].version).toBe('100.0.0')
})

test('installing configDependencies fails with --frozen-lockfile if env lockfile is missing', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/foo': '100.0.0+' + getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })

  const result = execPnpmSync(['install', '--frozen-lockfile'])
  expect(result.status).toBe(1)
  expect(result.stderr.toString()).toContain('Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date')
})

test('installing configDependencies succeeds with --frozen-lockfile if env lockfile is present and up-to-date', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/foo': '100.0.0+' + getIntegrity('@pnpm.e2e/foo', '100.0.0'),
    },
  })

  // First install to generate the env lockfile
  await execPnpm(['install'])

  // Second install with frozen-lockfile should succeed
  await execPnpm(['install', '--frozen-lockfile'])
})

test('pruning excludes preserves config dependencies and removes unused versions', async () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: { '@pnpm.e2e/foo': '~100.0.0' },
    minimumReleaseAgeExcludePrune: true,
    trustPolicyExcludePrune: true,
    minimumReleaseAgeExclude: ['@pnpm.e2e/foo', 'unused-plugin'],
    trustPolicyExclude: ['@pnpm.e2e/foo@99.0.0 || 100.0.0', 'unused-plugin'],
  })

  await execPnpm(['add', 'is-positive@1.0.0'])

  const settings = readYamlFileSync<{ minimumReleaseAgeExclude: string[], trustPolicyExclude: string[] }>('pnpm-workspace.yaml')
  expect(settings?.minimumReleaseAgeExclude).toEqual(['@pnpm.e2e/foo'])
  expect(settings?.trustPolicyExclude).toEqual(['@pnpm.e2e/foo@100.0.0'])
})
