import path from 'node:path'

import type { PnpmSettings } from '@pnpm/types'

import { getOptionsFromPnpmSettings, getOptionsFromRootManifest } from '../lib/getOptionsFromRootManifest.js'

const ORIGINAL_ENV = process.env

afterEach(() => {
  process.env = { ...ORIGINAL_ENV }
})

test('getOptionsFromRootManifest() should read "resolutions" field for compatibility with Yarn', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    resolutions: {
      foo: '1.0.0',
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() should read "overrides" field', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      overrides: {
        foo: '1.0.0',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by dependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    dependencies: {
      foo: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by devDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    devDependencies: {
      foo: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() Support $ in overrides by dependencies and devDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    dependencies: {
      foo: '1.0.0',
    },
    devDependencies: {
      foo: '2.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })
  expect(options.overrides).toStrictEqual({ foo: '1.0.0' })
})

test('getOptionsFromRootManifest() throws an error if cannot resolve an override version reference', () => {
  expect(() => getOptionsFromRootManifest(process.cwd(), {
    dependencies: {
      bar: '1.0.0',
    },
    pnpm: {
      overrides: {
        foo: '$foo',
      },
    },
  })).toThrow('Cannot resolve version $foo in overrides. The direct dependencies don\'t have dependency "foo".')
})

test('getOptionsFromRootManifest() should return allowBuilds as undefined by default', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {})
  expect(options.allowBuilds).toBeUndefined()
})

test('getOptionsFromRootManifest() should return allowBuilds', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowBuilds: { electron: true },
    },
  })
  expect(options.allowBuilds).toStrictEqual({ electron: true })
})

test('getOptionsFromRootManifest() should return patchedDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      patchedDependencies: {
        foo: 'foo.patch',
      },
    },
  })
  expect(options.patchedDependencies).toStrictEqual({ foo: path.resolve('foo.patch') })
})

test('getOptionsFromPnpmSettings() replaces env variables in settings', () => {
  process.env.PNPM_TEST_KEY = 'foo'
  process.env.PNPM_TEST_VALUE = 'bar'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    '${PNPM_TEST_KEY}': '${PNPM_TEST_VALUE}',
  } as any) as any // eslint-disable-line
  expect(options.foo).toBe('bar')
})

test('getOptionsFromPnpmSettings() throws a PnpmError when a string config value has an undefined env variable', () => {
  // Simulates runtime behavior when pnpm-workspace.yaml or config.yaml has a
  // string-valued setting referencing an env variable that is not defined.
  // The TypeScript type doesn't include all valid config keys, so we cast.
  const settings = { nodeLinker: '${PNPM_TEST_UNDEFINED_ENV_VAR_82645}' } as unknown as PnpmSettings
  expect(() => getOptionsFromPnpmSettings(process.cwd(), settings)).toThrow(
    expect.objectContaining({
      code: 'ERR_PNPM_CONFIG_UNRESOLVED_ENV_VAR',
    })
  )
})

test('getOptionsFromRootManifest() converts allowBuilds', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowBuilds: {
        foo: true,
        bar: false,
        qar: 'warn',
      },
    },
  })
  expect(options).toStrictEqual({
    allowBuilds: {
      foo: true,
      bar: false,
      qar: 'warn',
    },
  })
})
