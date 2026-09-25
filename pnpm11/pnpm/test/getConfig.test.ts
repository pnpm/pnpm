/// <reference path="../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { prepare } from '@pnpm/prepare'

jest.unstable_mockModule('@pnpm/installing.env-installer', () => ({
  resolveAndInstallConfigDeps: jest.fn(),
}))

jest.unstable_mockModule('@pnpm/store.connection-manager', () => ({
  createStoreController: jest.fn(),
}))

const loggerMock = {
  debug: jest.fn(),
  info: jest.fn(),
  warn: jest.fn(),
  error: jest.fn(),
}
jest.unstable_mockModule('@pnpm/logger', () => ({
  logger: Object.assign(jest.fn(() => loggerMock), loggerMock),
  globalWarn: jest.fn(),
  globalInfo: jest.fn(),
  globalError: jest.fn(),
}))

const { resolveAndInstallConfigDeps } = await import('@pnpm/installing.env-installer')
const { logger } = await import('@pnpm/logger')
const { createStoreController } = await import('@pnpm/store.connection-manager')
const { calcPnpmfilePathsOfPluginDeps, getConfig, installConfigDepsAndLoadHooks } = await import('../src/getConfig.js')

const storeCloseMock = jest.fn<() => Promise<void>>()

beforeEach(() => {
  jest.spyOn(console, 'warn')
  loggerMock.debug.mockClear()
  jest.mocked(resolveAndInstallConfigDeps).mockReset()
  storeCloseMock.mockReset().mockResolvedValue(undefined)
  jest.mocked(createStoreController).mockReset().mockResolvedValue({
    ctrl: { close: storeCloseMock },
    dir: '/tmp/store',
  } as unknown as Awaited<ReturnType<typeof createStoreController>>)
})

afterEach(() => {
  jest.mocked(console.warn).mockRestore()
})

test.each([undefined, '', 'dummy-token'])('emit trusted auth environment warnings for %p', async (token) => {
  prepare()
  const originalToken = process.env.PNPM_TEST_AUTH_TOKEN
  const originalAuthFile = process.env.PNPM_CONFIG_NPMRC_AUTH_FILE
  fs.writeFileSync('auth.npmrc', '//registry.example/:_authToken=${PNPM_TEST_AUTH_TOKEN}')
  process.env.PNPM_CONFIG_NPMRC_AUTH_FILE = path.resolve('auth.npmrc')
  if (token === undefined) delete process.env.PNPM_TEST_AUTH_TOKEN
  else process.env.PNPM_TEST_AUTH_TOKEN = token
  try {
    await getConfig({ json: false }, { workspaceDir: '.', excludeReporter: false })
    const warning = expect.stringContaining('Failed to replace env in config: ${PNPM_TEST_AUTH_TOKEN} in .npmrc key "_authToken"')
    if (token) expect(console.warn).not.toHaveBeenCalledWith(warning)
    else expect(console.warn).toHaveBeenCalledWith(warning)
    expect(console.warn).not.toHaveBeenCalledWith(expect.stringContaining('dummy-token'))
  } finally {
    if (originalToken === undefined) delete process.env.PNPM_TEST_AUTH_TOKEN
    else process.env.PNPM_TEST_AUTH_TOKEN = originalToken
    if (originalAuthFile === undefined) delete process.env.PNPM_CONFIG_NPMRC_AUTH_FILE
    else process.env.PNPM_CONFIG_NPMRC_AUTH_FILE = originalAuthFile
  }
})

test('console a warning when a project-level .npmrc has an unresolved env variable in an expanded setting', async () => {
  prepare()

  // `cafile` is still env-expanded (unlike registry/proxy URLs), so an
  // unresolved placeholder surfaces the generic env-replace warning.
  fs.writeFileSync('.npmrc', 'cafile=${ENV_VAR_123}', 'utf8')

  await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
  })

  expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('Failed to replace env in config: ${ENV_VAR_123}'))
})

test('console a warning when a project-level .npmrc uses an env variable in a request destination', async () => {
  prepare()

  // Env placeholders in repository-controlled registry/proxy URLs are not
  // expanded at all — the setting is dropped with a dedicated warning
  // instead of the generic env-replace one.
  fs.writeFileSync('.npmrc', 'registry=${ENV_VAR_123}', 'utf8')

  await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
  })

  expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('Ignored project-level request destination "registry"'))
})

describe('calcPnpmfilePathsOfPluginDeps', () => {
  test('yields pnpmfile.mjs when it exists', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const pluginDir = path.join(tmpDir, 'pnpm-plugin-foo')
      fs.mkdirSync(pluginDir, { recursive: true })
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.mjs'), '')
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.cjs'), '')

      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([path.join(pluginDir, 'pnpmfile.mjs')])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })

  test('falls back to pnpmfile.cjs when pnpmfile.mjs does not exist', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const pluginDir = path.join(tmpDir, 'pnpm-plugin-foo')
      fs.mkdirSync(pluginDir, { recursive: true })
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.cjs'), '')

      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([path.join(pluginDir, 'pnpmfile.cjs')])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })

  test('skips plugins whose directory is missing (e.g. config dep install never ran)', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })

  test('yields pnpmfile.cjs path even if missing when the plugin directory exists (so requireHooks reports the misconfiguration)', () => {
    const tmpDir = fs.mkdtempSync(path.join(import.meta.dirname, '.tmp-'))
    try {
      const pluginDir = path.join(tmpDir, 'pnpm-plugin-foo')
      fs.mkdirSync(pluginDir, { recursive: true })

      const paths = [...calcPnpmfilePathsOfPluginDeps(tmpDir, { 'pnpm-plugin-foo': '1.0.0' })]
      expect(paths).toEqual([path.join(pluginDir, 'pnpmfile.cjs')])
    } finally {
      fs.rmSync(tmpDir, { recursive: true })
    }
  })
})

test('hoist: false removes hoistPattern', async () => {
  prepare()

  const { config } = await getConfig({
    hoist: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
  })

  expect(config.hoist).toBe(false)
  expect(config.hoistPattern).toBeUndefined()
})

describe('installConfigDepsAndLoadHooks', () => {
  test('proceeds normally when configDependencies install succeeds', async () => {
    prepare()

    jest.mocked(resolveAndInstallConfigDeps).mockResolvedValueOnce(undefined as never)

    const { config, context } = buildBaseConfig()
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect(result).toBeDefined()
    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
    expect(loggerMock.debug).not.toHaveBeenCalled()
  })

  test('does not throw when install fails and tolerateConfigDependenciesErrors is true', async () => {
    prepare()

    const simulatedError = new Error('401 Unauthorized: missing auth token')
    jest.mocked(resolveAndInstallConfigDeps).mockRejectedValueOnce(simulatedError)

    const { config, context } = buildBaseConfig()

    const result = await installConfigDepsAndLoadHooks(config, context, {
      tolerateConfigDependenciesErrors: true,
    })

    expect(result).toBeDefined()
    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
    expect(logger.debug).toHaveBeenCalledWith(
      expect.objectContaining({
        message: expect.stringContaining('Failed to install configDependencies'),
        err: simulatedError,
      })
    )
  })

  test('throws when install fails and tolerateConfigDependenciesErrors is not set (default behaviour)', async () => {
    prepare()

    const simulatedError = new Error('401 Unauthorized: missing auth token')
    jest.mocked(resolveAndInstallConfigDeps).mockRejectedValueOnce(simulatedError)

    const { config, context } = buildBaseConfig()

    await expect(
      installConfigDepsAndLoadHooks(config, context)
    ).rejects.toThrow('401 Unauthorized: missing auth token')

    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
  })

  test('does not swallow store creation errors even when tolerateConfigDependenciesErrors is true', async () => {
    prepare()

    const storeError = new Error('EACCES: permission denied opening store dir')
    jest.mocked(createStoreController).mockRejectedValueOnce(storeError)

    const { config, context } = buildBaseConfig()

    await expect(
      installConfigDepsAndLoadHooks(config, context, { tolerateConfigDependenciesErrors: true })
    ).rejects.toThrow('EACCES: permission denied opening store dir')

    expect(resolveAndInstallConfigDeps).not.toHaveBeenCalled()
    expect(loggerMock.debug).not.toHaveBeenCalled()
  })

  test('does not swallow store close errors even when tolerateConfigDependenciesErrors is true', async () => {
    prepare()

    jest.mocked(resolveAndInstallConfigDeps).mockResolvedValueOnce(undefined as never)
    storeCloseMock.mockReset().mockRejectedValueOnce(new Error('store close failed'))

    const { config, context } = buildBaseConfig()

    await expect(
      installConfigDepsAndLoadHooks(config, context, { tolerateConfigDependenciesErrors: true })
    ).rejects.toThrow('store close failed')

    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
    expect(loggerMock.debug).not.toHaveBeenCalled()
  })

  test('forSelfUpdate does not load the default project pnpmfile', async () => {
    prepare()

    fs.writeFileSync('.pnpmfile.cjs', `
      require('fs').writeFileSync('pnpmfile-was-loaded', '')
      module.exports = { hooks: {} }
    `)

    const { config, context } = buildPnpmfileConfig()
    await installConfigDepsAndLoadHooks(config, context, { forSelfUpdate: true })

    // A repo-controlled .pnpmfile.cjs must not run during self-update — its
    // updateConfig hook or custom resolvers/fetchers could steer the fetch.
    expect(fs.existsSync('pnpmfile-was-loaded')).toBe(false)
    expect(context.hooks?.updateConfig ?? []).toHaveLength(0)
  })

  test('the default project pnpmfile is loaded when forSelfUpdate is not set', async () => {
    prepare()

    fs.writeFileSync('.pnpmfile.cjs', `
      require('fs').writeFileSync('pnpmfile-was-loaded', '')
      module.exports = { hooks: {} }
    `)

    const { config, context } = buildPnpmfileConfig()
    await installConfigDepsAndLoadHooks(config, context)

    expect(fs.existsSync('pnpmfile-was-loaded')).toBe(true)
  })

<<<<<<< HEAD
  test.each(['.pnpmfile.cjs', '.pnpmfile.mjs'])('the updateConfig hook of %s runs after the ones of config dependency plugins', async (defaultPnpmfile) => {
    prepare()

    jest.mocked(resolveAndInstallConfigDeps).mockResolvedValueOnce(undefined as never)
    for (const plugin of ['pnpm-plugin-a', 'pnpm-plugin-b']) {
      const pluginDir = path.join('node_modules/.pnpm-config', plugin)
      fs.mkdirSync(pluginDir, { recursive: true })
      fs.writeFileSync(path.join(pluginDir, 'pnpmfile.cjs'), `
        module.exports = { hooks: { updateConfig: (config) => ({ ...config, order: [...(config.order ?? []), '${plugin}'] }) } }
      `)
    }
    fs.writeFileSync(defaultPnpmfile, `
      const hooks = { updateConfig: (config) => ({ ...config, order: [...(config.order ?? []), 'project'] }) }
      ${defaultPnpmfile.endsWith('.mjs') ? 'export { hooks }' : 'module.exports = { hooks }'}
    `)

    const { config, context } = buildBaseConfig()
    config.ignorePnpmfile = false
    config.configDependencies = {
      'pnpm-plugin-b': '1.0.0+sha512-abc',
      'pnpm-plugin-a': '1.0.0+sha512-abc',
    }
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect((result.config as unknown as { order: string[] }).order).toStrictEqual(['pnpm-plugin-a', 'pnpm-plugin-b', 'project'])
  })

  test('an updateConfig hook that drops the default and @jsr routes falls back to the configured registry and the built-in @jsr route', async () => {
    prepare()

    fs.writeFileSync('.pnpmfile.cjs', `
      module.exports = {
        hooks: {
          updateConfig: (config) => ({ ...config, registriesByScope: { '@acme': 'https://acme.example' } }),
        },
      }
    `)

    const { config, context } = buildPnpmfileConfig()
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect(result.config.registriesByScope).toStrictEqual({
      default: 'https://mirror.example/',
      '@jsr': 'https://npm.jsr.io/',
      '@acme': 'https://acme.example/',
    })
    expect(result.config.registry).toBe('https://mirror.example/')
  })

  test('the default route an updateConfig hook sets becomes the registry', async () => {
    prepare()

    fs.writeFileSync('.pnpmfile.cjs', `
      module.exports = {
        hooks: {
          updateConfig: (config) => ({
            ...config,
            registriesByScope: { default: 'https://other-mirror.example/', '@jsr': 'https://jsr-mirror.example/' },
          }),
        },
      }
    `)

    const { config, context } = buildPnpmfileConfig()
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect(result.config.registriesByScope).toStrictEqual({
      default: 'https://other-mirror.example/',
      '@jsr': 'https://jsr-mirror.example/',
    })
    expect(result.config.registry).toBe('https://other-mirror.example/')
  })

  test('a scope an updateConfig hook unsets routes to the default registry', async () => {
    prepare()

    fs.writeFileSync('.pnpmfile.cjs', `
      module.exports = {
        hooks: {
          updateConfig: (config) => ({
            ...config,
            registriesByScope: { ...config.registriesByScope, '@acme': undefined, '@other': null },
          }),
        },
      }
    `)

    const { config, context } = buildPnpmfileConfig()
    config.registriesByScope['@acme'] = 'https://acme.example/'
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect(result.config.registriesByScope).toStrictEqual({
      default: 'https://mirror.example/',
      '@jsr': 'https://npm.jsr.io/',
    })
  })
  })

  function buildPnpmfileConfig (): { config: Config, context: ConfigContext } {
    const { config, context } = buildBaseConfig()
    config.ignorePnpmfile = false
    delete (config as { configDependencies?: unknown }).configDependencies
    config.registry = 'https://mirror.example/'
    config.registriesByScope = { default: 'https://mirror.example/', '@jsr': 'https://npm.jsr.io/' }
    return { config, context }
  }

  function buildBaseConfig (): { config: Config, context: ConfigContext } {
    const dir = process.cwd()
    const config = {
      ignorePnpmfile: true,
      configDependencies: { 'some-helper-pkg': '1.0.0+sha512-abc' },
      dir,
      lockfileDir: dir,
    } as unknown as Config
    const context = {
      rootProjectManifestDir: dir,
    } as unknown as ConfigContext
    return { config, context }
  }
})
