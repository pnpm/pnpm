import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { distTag } from '@pnpm/registry-access.commands'
import { REGISTRY_MOCK_CREDENTIALS, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { publish } from '@pnpm/releasing.commands'
import { DEFAULT_OPTS as BASE_OPTS } from '@pnpm/testing.command-defaults'

const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    creds: {
      basicAuth: REGISTRY_MOCK_CREDENTIALS,
    },
  },
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

test('dist-tag ls: should list dist-tags', async () => {
  const pkgName = 'test-dist-tag-ls'
  await publishVersion(pkgName, '1.0.0')

  const result = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['ls', pkgName])

  expect(result).toContain('latest: 1.0.0')
})

test('dist-tag ls: should list dist-tags without subcommand', async () => {
  const pkgName = 'test-dist-tag-ls-default'
  await publishVersion(pkgName, '1.0.0')

  const result = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, [pkgName])

  expect(result).toContain('latest: 1.0.0')
})

test('dist-tag add: should add a dist-tag', async () => {
  const pkgName = 'test-dist-tag-add'
  await publishVersion(pkgName, '1.0.0')

  const result = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['add', `${pkgName}@1.0.0`, 'beta'])

  expect(result).toBe(`+beta: ${pkgName}@1.0.0`)

  const lsResult = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['ls', pkgName])

  expect(lsResult).toContain('beta: 1.0.0')
  expect(lsResult).toContain('latest: 1.0.0')
})

test('dist-tag add: should default to latest tag', async () => {
  const pkgName = 'test-dist-tag-add-default'
  await publishVersion(pkgName, '1.0.0')
  await publishVersion(pkgName, '2.0.0')

  const result = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['add', `${pkgName}@1.0.0`])

  expect(result).toBe(`+latest: ${pkgName}@1.0.0`)
})

test('dist-tag rm: should remove a dist-tag', async () => {
  const pkgName = 'test-dist-tag-rm'
  await publishVersion(pkgName, '1.0.0')

  // First add a custom tag
  await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['add', `${pkgName}@1.0.0`, 'beta'])

  // Then remove it
  const result = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['rm', pkgName, 'beta'])

  expect(result).toBe(`-beta: ${pkgName}@1.0.0`)

  const lsResult = await distTag.handler({
    ...DEFAULT_OPTS,
    cliOptions: {},
    configByUri: CONFIG_BY_URI,
  }, ['ls', pkgName])

  expect(lsResult).not.toContain('beta')
  expect(lsResult).toContain('latest: 1.0.0')
})

test('dist-tag rm: should refuse to remove latest', async () => {
  const pkgName = 'test-dist-tag-rm-latest'
  await publishVersion(pkgName, '1.0.0')

  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, ['rm', pkgName, 'latest'])
  }).rejects.toThrow('Removing the "latest" dist-tag is not allowed')
})

test('dist-tag rm: should throw when tag does not exist', async () => {
  const pkgName = 'test-dist-tag-rm-missing'
  await publishVersion(pkgName, '1.0.0')

  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, ['rm', pkgName, 'nonexistent'])
  }).rejects.toThrow('dist-tag "nonexistent" is not set')
})

test('dist-tag ls: should throw when package not found', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['ls', 'nonexistent-pkg-dist-tag-99999'])
  }).rejects.toThrow('Package "nonexistent-pkg-dist-tag-99999" not found in registry')
})

test('dist-tag add: should throw when no version specified', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['add', 'some-package', 'beta'])
  }).rejects.toThrow('Version is required')
})

test('dist-tag ls: should throw when no package name provided', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['ls'])
  }).rejects.toThrow('Package name is required')
})

test('dist-tag rm: should throw when no arguments provided', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['rm'])
  }).rejects.toThrow('Package name and tag are required')
})

test('dist-tag add: should throw when no arguments provided', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['add'])
  }).rejects.toThrow('Package name and version are required')
})

test('dist-tag add: should reject non-exact semver versions', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
    }, ['add', 'some-package@^1.0.0', 'beta'])
  }).rejects.toThrow('Version must be an exact semver version')
})

test('dist-tag add: should throw when package not found', async () => {
  await expect(async () => {
    await distTag.handler({
      ...DEFAULT_OPTS,
      cliOptions: {},
      configByUri: CONFIG_BY_URI,
    }, ['add', 'nonexistent-pkg-dist-tag-add-99999@1.0.0', 'beta'])
  }).rejects.toThrow()
})
