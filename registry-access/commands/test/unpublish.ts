import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { unpublish } from '@pnpm/registry-access.commands'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { publish } from '@pnpm/releasing.commands'
import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'
import { safeExeca as execa } from 'execa'

const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    creds: {
      basicAuth: REGISTRY_MOCK_CREDENTIALS,
    },
  },
}

async function getVersions (pkgName: string): Promise<string[]> {
  try {
    const { stdout } = await execa('pnpm', [
      'view',
      pkgName,
      'versions',
      '--json',
      '--registry',
      REGISTRY,
    ])
    const parsed = JSON.parse(stdout?.toString() ?? '[]')
    if (typeof parsed === 'string') return [parsed]
    return parsed
  } catch {
    return []
  }
}

async function publishVersion (name: string, version: string): Promise<void> {
  prepare({
    name,
    version,
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
  }, [])
}

test('unpublish: should unpublish a specific version', async () => {
  const pkgName = 'test-unpublish-version'
  await publishVersion(pkgName, '0.0.1')
  await publishVersion(pkgName, '0.0.2')

  const result = await unpublish.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, [`${pkgName}@0.0.1`])

  expect(result).toContain('Successfully unpublished')
  expect(result).toContain('1 version(s)')

  const versions = await getVersions(pkgName)
  expect(versions).not.toContain('0.0.1')
  expect(versions).toContain('0.0.2')
})

test('unpublish: should unpublish entire package with --force', async () => {
  const pkgName = 'test-unpublish-force'
  await publishVersion(pkgName, '0.0.1')

  const result = await unpublish.handler({
    ...DEFAULT_OPTS,
    cliOptions: { force: true },
    configByUri: CONFIG_BY_URI,
  }, [pkgName])

  expect(result).toContain('Successfully unpublished')

  const versions = await getVersions(pkgName)
  expect(versions).toEqual([])
})

test('unpublish: should refuse to unpublish entire package without --force', async () => {
  const pkgName = 'test-unpublish-no-force'
  await publishVersion(pkgName, '0.0.1')

  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, [pkgName])
  }).rejects.toThrow('pnpm unpublish --force')
})

test('unpublish: should throw error when package not found', async () => {
  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['nonexistent-package-99999'])
  }).rejects.toThrow('Package "nonexistent-package-99999" not found in registry')
})

test('unpublish: should throw error when no package name provided', async () => {
  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, [])
  }).rejects.toThrow('Package name is required')
})

test('unpublish: should throw error when version not found', async () => {
  const pkgName = 'test-unpublish-no-ver'
  await publishVersion(pkgName, '0.0.1')

  await expect(async () => {
    await unpublish.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, [`${pkgName}@9.9.9`])
  }).rejects.toThrow('No versions match')
})
