import { beforeEach, expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

const { globalWarn } = await import('@pnpm/logger')
const { getOptionsFromPnpmSettings } = await import('../lib/getOptionsFromRootManifest.js')

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

test('getOptionsFromPnpmSettings() maps the "update" settings section to updateConfig', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {
      ignore: ['webpack', '@babel/*'],
    },
  })
  expect(options.updateConfig).toStrictEqual({
    ignoreDependencies: ['webpack', '@babel/*'],
  })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('getOptionsFromPnpmSettings() never leaks the raw "update" key into the options', () => {
  // The merged config uses `update` as the boolean that turns an install into
  // an update, so the settings object must not reach it under that key.
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {
      ignore: ['webpack'],
    },
  })
  expect('update' in options).toBe(false)
})

test('getOptionsFromPnpmSettings() accepts an empty "update" section', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {},
  })
  expect(options.updateConfig).toStrictEqual({})
  expect('update' in options).toBe(false)
})

test('getOptionsFromPnpmSettings() lets "update" win over "updateConfig" and warns', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {
      ignore: ['webpack'],
    },
    updateConfig: {
      ignoreDependencies: ['react'],
    },
  })
  expect(options.updateConfig).toStrictEqual({
    ignoreDependencies: ['webpack'],
  })
  expect(globalWarn).toHaveBeenCalledTimes(1)
  const warning = jest.mocked(globalWarn).mock.calls[0][0]
  expect(warning).toContain('update')
  expect(warning).toContain('updateConfig')
})

test('getOptionsFromPnpmSettings() still honors the deprecated "updateConfig" setting without warning', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    updateConfig: {
      ignoreDependencies: ['react'],
    },
  })
  expect(options.updateConfig).toStrictEqual({
    ignoreDependencies: ['react'],
  })
  expect(globalWarn).not.toHaveBeenCalled()
})
