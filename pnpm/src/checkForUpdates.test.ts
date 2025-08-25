import { getConfig } from '@pnpm/config'
import { prepareEmpty } from '@pnpm/prepare'
import { jest } from '@jest/globals'
import { loadJsonFile } from 'load-json-file'
import writeJsonFile from 'write-json-file'

const original = await import('@pnpm/core-loggers')
jest.unstable_mockModule('@pnpm/core-loggers', () => ({
  ...original,
  updateCheckLogger: { debug: jest.fn() },
}))

const { updateCheckLogger } = await import('@pnpm/core-loggers')
const { checkForUpdates } = await import('./checkForUpdates.js')

beforeEach(() => {
  jest.mocked(updateCheckLogger.debug).mockReset()
})

test('check for updates when no pnpm state file is present', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  await checkForUpdates({
    ...config,
    stateDir: process.cwd(),
  })

  expect(updateCheckLogger.debug).toHaveBeenCalledWith({
    currentVersion: expect.any(String),
    latestVersion: expect.any(String),
  })

  const state = loadJsonFile.sync('pnpm-state.json')
  expect(state).toEqual({
    lastUpdateCheck: expect.any(String),
  })
})

test('do not check for updates when last update check happened recently', async () => {
  prepareEmpty()

  const lastUpdateCheck = new Date().toUTCString()
  writeJsonFile.sync('pnpm-state.json', { lastUpdateCheck })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  await checkForUpdates({
    ...config,
    stateDir: process.cwd(),
  })

  expect(updateCheckLogger.debug).not.toHaveBeenCalled()

  const state = loadJsonFile.sync('pnpm-state.json')
  expect(state).toStrictEqual({ lastUpdateCheck })
})

test('check for updates when last update check happened two days ago', async () => {
  prepareEmpty()

  const lastUpdateCheckDate = new Date()
  lastUpdateCheckDate.setDate(lastUpdateCheckDate.getDate() - 2)
  const initialLastUpdateCheck = lastUpdateCheckDate.toUTCString()
  writeJsonFile.sync('pnpm-state.json', {
    lastUpdateCheck: initialLastUpdateCheck,
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  await checkForUpdates({
    ...config,
    stateDir: process.cwd(),
  })

  expect(updateCheckLogger.debug).toHaveBeenCalledWith({
    currentVersion: expect.any(String),
    latestVersion: expect.any(String),
  })

  const state = loadJsonFile.sync<{ lastUpdateCheck: string }>('pnpm-state.json')
  expect(state.lastUpdateCheck).toBeDefined()
  expect(state.lastUpdateCheck).not.toEqual(initialLastUpdateCheck)
})
