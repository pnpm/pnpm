import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { deprecate, undeprecate } from '@pnpm/registry-access.commands'
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

async function getDeprecation (pkgName: string): Promise<string | undefined> {
  const { stdout } = await execa('pnpm', [
    'view',
    `${pkgName}`,
    'deprecated',
    '--json',
    '--registry',
    REGISTRY,
  ])
  try {
    return JSON.parse(stdout?.toString() ?? 'null')
  } catch {
    return undefined
  }
}

test('deprecate: should deprecate a package', async () => {
  prepare({
    name: 'test-deprecate-package',
    version: '0.0.1',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['test-deprecate-package', 'This package is deprecated'])

  const deprecated = await getDeprecation('test-deprecate-package')
  expect(deprecated).toBe('This package is deprecated')
})

test('deprecate: should deprecate a specific version', async () => {
  prepare({
    name: 'test-deprecate-specific',
    version: '0.0.1',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['test-deprecate-specific@0.0.1', 'This version is deprecated'])

  const deprecated = await getDeprecation('test-deprecate-specific')
  expect(deprecated).toBe('This version is deprecated')
})

test('undeprecate: should undeprecate a package', async () => {
  prepare({
    name: 'test-undeprecate-pkg',
    version: '0.0.1',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['test-undeprecate-pkg', 'This package is deprecated'])

  await undeprecate.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['test-undeprecate-pkg'])

  const deprecated = await getDeprecation('test-undeprecate-pkg')
  expect(deprecated).toBeFalsy()
})

test('deprecate: should throw error when package not found', async () => {
  await expect(async () => {
    await deprecate.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['nonexistent-package-12345', 'This should fail'])
  }).rejects.toThrow('Package "nonexistent-package-12345" not found in registry')
})

test('deprecate: should throw error when no package name provided', async () => {
  await expect(async () => {
    await deprecate.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, [])
  }).rejects.toThrow('Package name is required')
})
