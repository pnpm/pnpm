import { afterEach, expect, test } from '@jest/globals'
import type { PackageExtension } from '@pnpm/types'

import { getOptionsFromPnpmSettings } from '../lib/getOptionsFromRootManifest.js'

const ORIGINAL_ENV = process.env

afterEach(() => {
  process.env = { ...ORIGINAL_ENV }
})

test('getOptionsFromPnpmSettings() replaces env variables in settings', () => {
  process.env.PNPM_TEST_KEY = 'foo'
  process.env.PNPM_TEST_VALUE = 'bar'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    '${PNPM_TEST_KEY}': '${PNPM_TEST_VALUE}',
  } as any) as any // eslint-disable-line
  expect(options.foo).toBe('bar')
})

test('getOptionsFromPnpmSettings() ignores env variables inside registries values', () => {
  process.env.PNPM_TEST_TOKEN = 'secret'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      default: 'https://registry.npmjs.org/',
      '@scope': 'https://registry.example.com/${PNPM_TEST_TOKEN}/',
    },
  }) as any // eslint-disable-line
  expect(options.registries).toStrictEqual({
    default: 'https://registry.npmjs.org/',
  })
})

test('getOptionsFromPnpmSettings() ignores env variables inside namedRegistries values', () => {
  process.env.PNPM_TEST_HOST = 'work.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    namedRegistries: {
      work: 'https://${PNPM_TEST_HOST}/npm/',
    },
  } as any) as any // eslint-disable-line
  expect(options.namedRegistries).toStrictEqual({})
})

test('getOptionsFromPnpmSettings() ignores env variables inside registry setting', () => {
  process.env.PNPM_TEST_HOST = 'registry.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registry: 'https://${PNPM_TEST_HOST}/npm/',
  } as any) as any // eslint-disable-line
  expect(options.registry).toBeUndefined()
})

test('getOptionsFromPnpmSettings() ignores env variables inside pnprServer setting', () => {
  process.env.PNPM_TEST_HOST = 'registry.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    pnprServer: 'https://${PNPM_TEST_HOST}/pnpr/',
  } as any) as any // eslint-disable-line
  expect(options.pnprServer).toBeUndefined()
})

test('getOptionsFromPnpmSettings() may expand env variables inside trusted request destinations', () => {
  process.env.PNPM_TEST_HOST = 'registry.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    pnprServer: 'https://${PNPM_TEST_HOST}/pnpr/',
    registry: 'https://${PNPM_TEST_HOST}/npm/',
    registries: {
      '@scope': 'https://${PNPM_TEST_HOST}/scope/',
    },
    namedRegistries: {
      work: 'https://${PNPM_TEST_HOST}/work/',
    },
  } as any, { expandRequestDestinationEnv: true }) as any // eslint-disable-line
  expect(options.pnprServer).toBe('https://registry.example.com/pnpr/')
  expect(options.registry).toBe('https://registry.example.com/npm/')
  expect(options.registries).toStrictEqual({
    '@scope': 'https://registry.example.com/scope/',
  })
  expect(options.namedRegistries).toStrictEqual({
    work: 'https://registry.example.com/work/',
  })
})

test('getOptionsFromPnpmSettings() converts allowBuilds', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    allowBuilds: {
      foo: true,
      bar: false,
      qar: 'warn',
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

test('getOptionsFromPnpmSettings() rejects non-string overrides values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      foo: null,
    } as unknown as Record<string, string>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_OVERRIDES',
    message: 'The value of overrides.foo should be a string, but got null',
  }))
})

test('getOptionsFromPnpmSettings() rejects array overrides values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      foo: [],
    } as unknown as Record<string, string>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_OVERRIDES',
    message: 'The value of overrides.foo should be a string, but got array',
  }))
})

test('getOptionsFromPnpmSettings() rejects non-object overrides values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    overrides: [] as unknown as Record<string, string>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_OVERRIDES',
    message: 'The overrides field should be an object, but got array',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-string range in packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        peerDependencies: {
          bar: null,
        },
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].peerDependencies.bar" setting should be a string, but got null',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-boolean optional flag in packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        peerDependenciesMeta: {
          bar: {
            optional: 'yes',
          },
        },
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].peerDependenciesMeta.bar.optional" setting should be a boolean, but got string',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-object package extension', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': [],
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\']" setting should be an object, but got array',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-object dependency field in packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        dependencies: [],
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].dependencies" setting should be an object, but got array',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-string range in optionalDependencies of packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        optionalDependencies: {
          bar: 1,
        },
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].optionalDependencies.bar" setting should be a string, but got number',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-object peerDependenciesMeta in packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        peerDependenciesMeta: 'bar',
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].peerDependenciesMeta" setting should be an object, but got string',
  }))
})

test('getOptionsFromPnpmSettings() rejects a non-object peerDependenciesMeta entry in packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        peerDependenciesMeta: {
          bar: true,
        },
      },
    } as unknown as Record<string, PackageExtension>,
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions[\'foo@*\'].peerDependenciesMeta.bar" setting should be an object, but got boolean',
  }))
})

test('getOptionsFromPnpmSettings() rejects non-object packageExtensions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: false,
  } as unknown as Record<string, unknown>)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: 'The "packageExtensions" setting should be an object, but got boolean',
  }))
})

// A key left empty in pnpm-workspace.yaml parses to null. pacquet reads the same
// shapes into `Option` fields, where null and an absent key are the same thing.
// The nulls are passed through rather than stripped: every reader of these
// fields already treats them as unset, so normalizing them here would only add a
// second spelling of the same state.
test('getOptionsFromPnpmSettings() accepts null packageExtensions fields', () => {
  expect(getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        dependencies: null,
        peerDependenciesMeta: {
          bar: {
            optional: null,
          },
        },
      },
    },
  } as unknown as Record<string, unknown>).packageExtensions).toStrictEqual({
    'foo@*': {
      dependencies: null,
      peerDependenciesMeta: {
        bar: {
          optional: null,
        },
      },
    },
  })
})

test('getOptionsFromPnpmSettings() accepts valid packageExtensions', () => {
  expect(getOptionsFromPnpmSettings(process.cwd(), {
    packageExtensions: {
      'foo@*': {
        dependencies: {
          foo: '1.0.0',
        },
        peerDependencies: {
          bar: '*',
        },
        peerDependenciesMeta: {
          bar: {
            optional: true,
          },
        },
      },
    },
  }).packageExtensions).toStrictEqual({
    'foo@*': {
      dependencies: {
        foo: '1.0.0',
      },
      peerDependencies: {
        bar: '*',
      },
      peerDependenciesMeta: {
        bar: {
          optional: true,
        },
      },
    },
  })
})
