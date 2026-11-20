import fs from 'node:fs'

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

  writeYamlFileSync('pnpm-workspace.yaml', { ...settings, configDependencies: {} })
  await execPnpm(['add', 'is-negative@1.0.0'])

  const afterRemoval = readYamlFileSync<Record<string, unknown>>('pnpm-workspace.yaml')
  expect(afterRemoval).toMatchObject({ minimumReleaseAgeExcludePrune: true, trustPolicyExcludePrune: true })
  expect(afterRemoval?.minimumReleaseAgeExclude).toBeUndefined()
  expect(afterRemoval?.trustPolicyExclude).toBeUndefined()
})

test.each([{}, undefined])('reconciles removal of the last config dependency (%p)', async (configDependencies) => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    configDependencies: { '@pnpm.e2e/foo': '100.0.0' },
  })
  await execPnpm(['install'])
  const installed = 'node_modules/.pnpm-config/@pnpm.e2e/foo/package.json'
  expect(fs.existsSync(installed)).toBe(true)
  const before = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', configDependencies ? { configDependencies } : {})
  await execPnpm(['list', '--json'])
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(before)

  const frozen = execPnpmSync(['install', '--frozen-lockfile'])
  expect(frozen.status).toBe(1)
  expect(frozen.stderr.toString()).toContain('Cannot update configDependencies with "frozen-lockfile"')
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(before)
  expect(fs.existsSync(installed)).toBe(true)

  await execPnpm(['install', '--offline'])
  const env = await readEnvLockfile(process.cwd())
  expect(env?.importers['.'].configDependencies).toEqual({})
  expect(env?.snapshots).toEqual({})
  expect(env?.packages).toEqual({})
  expect(fs.existsSync(installed)).toBe(false)
  await execPnpm(['install', '--frozen-lockfile', '--offline'])
})

test('cleans stale config dependency links without an env lockfile', async () => {
  prepare()
  const stale = 'node_modules/.pnpm-config/@example/removed'
  fs.mkdirSync(stale, { recursive: true })
  await execPnpm(['install', '--offline'])
  expect(fs.existsSync(stale)).toBe(false)
})
