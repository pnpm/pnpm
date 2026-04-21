import { expect, test } from '@jest/globals'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { prepare } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
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
