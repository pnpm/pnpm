import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { deprecate, publish } from '@pnpm/releasing.commands'
import { safeExeca as execa } from 'execa'

import { DEFAULT_OPTS } from './utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}`

const CREDENTIALS = [
  `--registry=${REGISTRY}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:password=password`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]

async function getDeprecation (pkgName: string, _version: string): Promise<string | undefined> {
  const { stdout } = await execa('npm', [
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
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['deprecate', ...CREDENTIALS] },
    cliOptions: {},
    rawConfig: { registry: REGISTRY },
    registry: REGISTRY,
  }, ['test-deprecate-package', 'This package is deprecated'])

  const deprecated = await getDeprecation('test-deprecate-package', '0.0.1')
  expect(deprecated).toBe('This package is deprecated')
})

test('deprecate: should deprecate a specific version', async () => {
  prepare({
    name: 'test-deprecate-specific',
    version: '0.0.1',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['deprecate', ...CREDENTIALS] },
    cliOptions: {},
    rawConfig: { registry: REGISTRY },
    registry: REGISTRY,
  }, ['test-deprecate-specific@0.0.1', 'This version is deprecated'])

  const deprecated = await getDeprecation('test-deprecate-specific', '0.0.1')
  expect(deprecated).toBe('This version is deprecated')
})

test.skip('deprecate: should undeprecate a package with empty message', async () => {
  prepare({
    name: 'test-undeprecate-package',
    version: '0.0.1',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['deprecate', ...CREDENTIALS] },
    cliOptions: {},
    rawConfig: { registry: REGISTRY },
    registry: REGISTRY,
  }, ['test-undeprecate-package', 'This package is deprecated'])

  await deprecate.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['deprecate', ...CREDENTIALS] },
    cliOptions: {},
    rawConfig: { registry: REGISTRY },
    registry: REGISTRY,
  }, ['test-undeprecate-package', ''])

  const deprecated = await getDeprecation('test-undeprecate-package', '0.0.1')
  expect(deprecated).toBeFalsy()
})

test('deprecate: should throw error when package not found', async () => {
  await expect(async () => {
    await deprecate.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['deprecate', ...CREDENTIALS] },
      cliOptions: {},
      rawConfig: { registry: REGISTRY },
      registry: REGISTRY,
    }, ['nonexistent-package-12345', 'This should fail'])
  }).rejects.toThrow('Package "nonexistent-package-12345" not found in registry')
})

test('deprecate: should throw error when no package name provided', async () => {
  await expect(async () => {
    await deprecate.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['deprecate', ...CREDENTIALS] },
      cliOptions: {},
      rawConfig: { registry: REGISTRY },
      registry: REGISTRY,
    }, [])
  }).rejects.toThrow('Package name is required')
})
