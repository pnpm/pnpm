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
  createStoreController: jest.fn().mockImplementation(async () => ({
    ctrl: {
      close: jest.fn<() => Promise<void>>().mockResolvedValue(undefined),
    },
    dir: '/tmp/store',
  })),
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
const { calcPnpmfilePathsOfPluginDeps, getConfig, installConfigDepsAndLoadHooks } = await import('../src/getConfig.js')

beforeEach(() => {
  jest.spyOn(console, 'warn')
  loggerMock.debug.mockClear()
  jest.mocked(resolveAndInstallConfigDeps).mockReset()
})

afterEach(() => {
  jest.mocked(console.warn).mockRestore()
})

test('console a warning when the .npmrc has an env variable that does not exist', async () => {
  prepare()

  fs.writeFileSync('.npmrc', 'registry=${ENV_VAR_123}', 'utf8')

  await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
  })

  expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('Failed to replace env in config: ${ENV_VAR_123}'))
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

  test('proceeds normally when configDependencies install succeeds', async () => {
    prepare()

    jest.mocked(resolveAndInstallConfigDeps).mockResolvedValueOnce(undefined as never)

    const { config, context } = buildBaseConfig()
    const result = await installConfigDepsAndLoadHooks(config, context)

    expect(result).toBeDefined()
    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
    expect(loggerMock.debug).not.toHaveBeenCalled()
  })

  test('does not throw when install fails and catchConfigDependenciesErrors is true', async () => {
    prepare()

    const simulatedError = new Error('401 Unauthorized: missing auth token')
    jest.mocked(resolveAndInstallConfigDeps).mockRejectedValueOnce(simulatedError)

    const { config, context } = buildBaseConfig()

    const result = await installConfigDepsAndLoadHooks(config, context, {
      catchConfigDependenciesErrors: true,
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

  test('throws when install fails and catchConfigDependenciesErrors is not set (default behaviour)', async () => {
    prepare()

    const simulatedError = new Error('401 Unauthorized: missing auth token')
    jest.mocked(resolveAndInstallConfigDeps).mockRejectedValueOnce(simulatedError)

    const { config, context } = buildBaseConfig()

    await expect(
      installConfigDepsAndLoadHooks(config, context)
    ).rejects.toThrow('401 Unauthorized: missing auth token')

    expect(resolveAndInstallConfigDeps).toHaveBeenCalledTimes(1)
  })
})
