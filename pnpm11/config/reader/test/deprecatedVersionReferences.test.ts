import { beforeEach, expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/logger', () => ({
  globalWarn: jest.fn(),
}))

const { globalWarn } = await import('@pnpm/logger')
const { getOptionsFromPnpmSettings } = await import('../lib/getOptionsFromRootManifest.js')

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

test('getOptionsFromPnpmSettings() warns about deprecated "$" version references and still resolves them', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      foo: '$foo',
    },
  }, {
    dependencies: {
      foo: '^1.0.0',
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '^1.0.0' })
  expect(globalWarn).toHaveBeenCalledTimes(1)
  const warning = jest.mocked(globalWarn).mock.calls[0][0]
  expect(warning).toContain('deprecated')
  expect(warning).toContain('foo')
  expect(warning).toContain('catalog:')
})

test('getOptionsFromPnpmSettings() does not warn when no "$" version references are used', () => {
  getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      foo: '^1.0.0',
      bar: 'catalog:',
    },
  }, {
    dependencies: {
      foo: '^1.0.0',
    },
  })
  expect(globalWarn).not.toHaveBeenCalled()
})

test('getOptionsFromPnpmSettings() does not warn for "${VAR}" env placeholders in resolutions', () => {
  // `${VAR}` is the env-placeholder syntax, not a `$dep` version reference.
  // Manifest resolutions preserve them literally (no env expansion —
  // secrets must not leak into the lockfile), so the deprecated-`$`-syntax
  // warning must not fire for them.
  getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {
      foo: '${SOME_ENV_VAR}',
    },
  } as any) // eslint-disable-line
  expect(globalWarn).not.toHaveBeenCalled()
})
