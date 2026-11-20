import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { deprecate, undeprecate } from '@pnpm/registry-access.commands'
import { publish } from '@pnpm/releasing.commands'
import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import { getRegistryMockToken, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { safeExeca as execa } from 'execa'

const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    '@': { authToken: getRegistryMockToken() },
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

test('deprecate and undeprecate name their command on the packument PUT', async () => {
  await setupMockAgent()
  try {
    const registryUrl = 'https://registry.example.com/'
    const packument = (deprecated?: string) => ({ name: 'pkg', versions: { '1.0.0': { name: 'pkg', version: '1.0.0', deprecated } } })
    const mockPool = getMockAgent().get('https://registry.example.com')
    mockPool.intercept({ method: 'GET', path: '/pkg?write=true' }).reply(200, packument())
    mockPool.intercept({ method: 'GET', path: '/pkg?write=true' }).reply(200, packument('old'))
    mockPool.intercept({ method: 'PUT', path: '/pkg', headers: { 'npm-command': 'deprecate' } }).reply(200, {})
    mockPool.intercept({ method: 'PUT', path: '/pkg', headers: { 'npm-command': 'undeprecate' } }).reply(200, {})

    const opts = { cliOptions: {}, registriesByScope: { default: registryUrl } }
    await expect(deprecate.handler(opts, ['pkg', 'old'])).resolves.toContain('deprecated 1 version(s)')
    await expect(undeprecate.handler(opts, ['pkg'])).resolves.toContain('un-deprecated 1 version(s)')
  } finally {
    await teardownMockAgent()
  }
})
