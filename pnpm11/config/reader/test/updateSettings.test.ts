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
      ignoreDeps: ['webpack', '@babel/*'],
    },
  })
  expect(options.updateConfig).toStrictEqual({
    ignoreDependencies: ['webpack', '@babel/*'],
  })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('getOptionsFromPnpmSettings() maps "update.changeset" to updateConfig.changeset', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {
      changeset: true,
      ignoreDeps: ['webpack'],
    },
  })
  expect(options.updateConfig).toStrictEqual({
    changeset: true,
    ignoreDependencies: ['webpack'],
  })
})

test('getOptionsFromPnpmSettings() throws when "update.changeset" is not a boolean', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    update: { changeset: 'yes' },
  } as any)).toThrow(/update\.changeset/) // eslint-disable-line
})

test('getOptionsFromPnpmSettings() never leaks the raw "update" key into the options', () => {
  // The merged config uses `update` as the boolean that turns an install into
  // an update, so the settings object must not reach it under that key.
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    update: {
      ignoreDeps: ['webpack'],
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
      ignoreDeps: ['webpack'],
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

test('getOptionsFromPnpmSettings() maps the "audit" settings section to auditConfig and auditLevel', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    audit: {
      level: 'high',
      ignore: ['GHSA-1', 'GHSA-2'],
    },
  }) as any // eslint-disable-line
  expect(options.auditConfig).toStrictEqual({ ignoreGhsas: ['GHSA-1', 'GHSA-2'] })
  expect(options.auditLevel).toBe('high')
  expect(globalWarn).not.toHaveBeenCalled()
})

test('getOptionsFromPnpmSettings() never leaks the raw "audit" key into the options', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    audit: {
      ignore: ['GHSA-1'],
    },
  })
  expect('audit' in options).toBe(false)
})

test('getOptionsFromPnpmSettings() lets "audit" win over "auditConfig" and "auditLevel" and warns', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    audit: {
      level: 'critical',
      ignore: ['GHSA-new'],
    },
    auditConfig: {
      ignoreGhsas: ['GHSA-old'],
    },
    auditLevel: 'low',
  } as any) as any // eslint-disable-line
  expect(options.auditConfig).toStrictEqual({ ignoreGhsas: ['GHSA-new'] })
  expect(options.auditLevel).toBe('critical')
  expect(globalWarn).toHaveBeenCalledTimes(2)
})

test('getOptionsFromPnpmSettings() still honors the deprecated "auditConfig" setting without warning', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    auditConfig: {
      ignoreGhsas: ['GHSA-old'],
    },
  })
  expect(options.auditConfig).toStrictEqual({ ignoreGhsas: ['GHSA-old'] })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('getOptionsFromPnpmSettings() throws when "update.ignoreDeps" is not a string array', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    update: { ignoreDeps: 'webpack' },
  } as any)).toThrow(/update\.ignoreDeps/) // eslint-disable-line
})

test('getOptionsFromPnpmSettings() throws when "audit.ignore" is not a string array', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    audit: { ignore: 'GHSA-1' },
  } as any)).toThrow(/audit\.ignore/) // eslint-disable-line
})

test('getOptionsFromPnpmSettings() throws on an invalid "audit.level"', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    audit: { level: 'severe' },
  } as any)).toThrow(/audit\.level/) // eslint-disable-line
})

test('getOptionsFromPnpmSettings() throws when the "update" section is not an object', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    update: 'webpack',
  } as any)).toThrow(/"update" setting should be an object/) // eslint-disable-line
})

test('getOptionsFromPnpmSettings() throws when the "audit" section is not an object', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    audit: true,
  } as any)).toThrow(/"audit" setting should be an object/) // eslint-disable-line
})
