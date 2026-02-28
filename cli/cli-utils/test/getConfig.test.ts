/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import { jest } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

jest.unstable_mockModule('@pnpm/config.deps-installer', () => ({
  installConfigDeps: jest.fn(),
}))

jest.unstable_mockModule('@pnpm/logger', () => {
  const mockMethods = {
    debug: jest.fn(),
    info: jest.fn(),
    warn: jest.fn(),
    error: jest.fn(),
  }

  const logger = Object.assign(
    jest.fn().mockReturnValue(mockMethods),
    mockMethods
  )

  return {
    logger,
    globalWarn: jest.fn(),
    globalInfo: jest.fn(),
    globalError: jest.fn(),
  }
})

jest.unstable_mockModule('@pnpm/store-connection-manager', () => ({
  createStoreController: jest.fn().mockImplementation(async () => ({
    ctrl: {
      close: jest.fn<() => Promise<void>>().mockResolvedValue(undefined),
    },
  })),
}))

const { installConfigDeps } = await import('@pnpm/config.deps-installer')
const { logger } = await import('@pnpm/logger')
const { getConfig } = await import('@pnpm/cli-utils')

beforeEach(() => {
  jest.spyOn(console, 'warn')
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
    rcOptionsTypes: {},
  })

  expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('Failed to replace env in config: ${ENV_VAR_123}'))
})

test('hoist: false removes hoistPattern', async () => {
  prepare()

  const config = await getConfig({
    hoist: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  expect(config.hoist).toBe(false)
  expect(config.hoistPattern).toBeUndefined()
})

test('proceeds normally when configDependencies install successfully', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'configDependencies:\n  some-helper-pkg: "1.0.0"', 'utf8')

  jest.mocked(installConfigDeps).mockResolvedValueOnce(undefined)

  const config = await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
  })

  expect(config).toBeDefined()
  expect(installConfigDeps).toHaveBeenCalled()
})

test('does not crash when configDependencies fail to install (e.g. missing auth token)', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'configDependencies:\n  some-helper-pkg: "1.0.0"', 'utf8')

  const simulatedError = new Error('401 Unauthorized: missing auth token')

  jest.mocked(installConfigDeps).mockRejectedValueOnce(simulatedError)

  const config = await getConfig({
    json: false,
  }, {
    workspaceDir: '.',
    excludeReporter: false,
    rcOptionsTypes: {},
    catchConfigDependenciesErrors: true,
  })

  expect(config).toBeDefined()
  expect(installConfigDeps).toHaveBeenCalled()
  expect(logger.debug).toHaveBeenCalledWith(
    expect.objectContaining({
      message: expect.stringContaining('Failed to install configDependencies'),
      err: simulatedError,
    })
  )
})

test('throws error when configDependencies fail and catchConfigDependenciesErrors is false (default)', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'configDependencies:\n  some-helper-pkg: "1.0.0"', 'utf8')

  const simulatedError = new Error('401 Unauthorized: missing auth token')
  jest.mocked(installConfigDeps).mockRejectedValueOnce(simulatedError)

  await expect(
    getConfig({
      json: false,
    }, {
      workspaceDir: '.',
      excludeReporter: false,
      rcOptionsTypes: {},
    })
  ).rejects.toThrow('401 Unauthorized: missing auth token')

  expect(installConfigDeps).toHaveBeenCalled()
})
