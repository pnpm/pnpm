import util from 'node:util'

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
  expect(options.registriesByScope).toStrictEqual({
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
  expect(options.registriesByPrefix).toBeUndefined()
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
  expect(options.registriesByScope).toStrictEqual({
    '@scope': 'https://registry.example.com/scope/',
  })
  expect(options.registriesByPrefix).toStrictEqual({
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

test('getOptionsFromPnpmSettings() keys the registry options by normalized registry URL', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://artifactory.example/artifactory/api/npm/npm-virtual': { serverType: 'artifactory' },
    },
  })
  expect(options.registryOptionsByUrl).toStrictEqual({
    'https://artifactory.example/artifactory/api/npm/npm-virtual/': { serverType: 'artifactory' },
  })
})

test('getOptionsFromPnpmSettings() rejects an unknown registry server type', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.example.com/': { serverType: 'nexus' as never },
    },
  })).toThrow(/should be one of "npm", "artifactory", but got "nexus"/)
})

test('getOptionsFromPnpmSettings() rejects credentials in a registry declaration', () => {
  // pnpm-workspace.yaml is committed; credentials belong in .npmrc.
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.example.com/': { _authToken: 'secret' } as never,
    },
  })).toThrow(/is not allowed in pnpm-workspace\.yaml/)
})

test('getOptionsFromPnpmSettings() rejects a URL-keyed registries entry written as a string', () => {
  // A map of strings is the `<scope>: <url>` shape, and a URL is not a scope,
  // so this one would otherwise be read as a route that can never match.
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.example.com/': 'artifactory' as never,
    },
  })).toThrow(/is a string/)
})

test('getOptionsFromPnpmSettings() rejects a registries map that mixes both shapes', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      '@acme': 'https://npm.example.com/',
      'https://artifactory.example/': { serverType: 'artifactory' },
    } as never,
  })).toThrow(/mixes registry declarations with "<scope>: <url>" entries \("@acme"\)/)
})

test('getOptionsFromPnpmSettings() drops a registries entry whose URL has an unexpanded env placeholder', () => {
  // The key is a request destination, so it gets the same gate the `registries`
  // values get: no silent expansion outside a trusted context.
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://${PNPM_TEST_HOST}/': { serverType: 'artifactory' },
    },
  })
  expect(options.registryOptionsByUrl).toBeUndefined()
})

test('getOptionsFromPnpmSettings() expands the registries key when request destinations may be expanded', () => {
  process.env.PNPM_TEST_HOST = 'artifactory.example'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://${PNPM_TEST_HOST}/': { serverType: 'artifactory' },
    },
  }, { expandRequestDestinationEnv: true })
  expect(options.registryOptionsByUrl).toStrictEqual({
    'https://artifactory.example/': { serverType: 'artifactory' },
  })
})

test('getOptionsFromPnpmSettings() rejects credentials embedded in a registries key', () => {
  // pnpm-workspace.yaml is committed; credentials belong in .npmrc.
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://ci-user-6e42:hunter2@npm.example.com/': { serverType: 'artifactory' },
    },
  })).toThrow(/key embeds credentials/)
})

test('getOptionsFromPnpmSettings() does not mistake an @ later in the path for credentials', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.example.com/scope@1/': { serverType: 'artifactory' },
    },
  })
  expect(options.registryOptionsByUrl).toStrictEqual({
    'https://npm.example.com/scope@1/': { serverType: 'artifactory' },
  })
})

test('getOptionsFromPnpmSettings() rejects credentials in a scheme-less registries key', () => {
  // `.npmrc` scopes settings with a scheme-less `//host/`, and this setting's
  // own error points users at that syntax, so it is the form they are most
  // likely to write.
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      '//ci-user-6e42:hunter2@npm.example.com/': { serverType: 'artifactory' },
    },
  })).toThrow(/key embeds credentials/)

  // The message names the key, so it must not carry the credentials with it.
  try {
    getOptionsFromPnpmSettings(process.cwd(), {
      registries: {
        '//ci-user-6e42:hunter2@npm.example.com/': { serverType: 'artifactory' },
      },
    })
  } catch (err) {
    const message = util.types.isNativeError(err) ? err.message : String(err)
    expect(message).not.toContain('hunter2')
    expect(message).not.toContain('ci-user-6e42')
    expect(message).toContain('npm.example.com')
  }
})

test('getOptionsFromPnpmSettings() accepts a scheme-less registries key without credentials', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      '//npm.example.com/': { serverType: 'artifactory' },
    },
  })
  expect(options.registryOptionsByUrl).toStrictEqual({
    '//npm.example.com/': { serverType: 'artifactory' },
  })
})

test('getOptionsFromPnpmSettings() is not fooled by a :// later in a scheme-less registries key', () => {
  // Searching for the first `://` would find the one in the path and parse the
  // authority from there, leaving the real credentials unexamined.
  const key = '//ci-user-6e42:hunter2@npm.example.com/a://b'
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: { [key]: { serverType: 'artifactory' } },
  })).toThrow(/key embeds credentials/)

  try {
    getOptionsFromPnpmSettings(process.cwd(), {
      registries: { [key]: { serverType: 'artifactory' } },
    })
  } catch (err) {
    const message = util.types.isNativeError(err) ? err.message : String(err)
    expect(message).not.toContain('hunter2')
    expect(message).not.toContain('ci-user-6e42')
  }
})

test('getOptionsFromPnpmSettings() routes the declared scopes to the registry', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example': { scopes: ['@', '@foo', '@bar'], serverType: 'npm' },
    },
  }) as any // eslint-disable-line
  expect(options.registriesByScope).toStrictEqual({
    default: 'https://npm.corp.example/',
    '@foo': 'https://npm.corp.example/',
    '@bar': 'https://npm.corp.example/',
  })
})

test('getOptionsFromPnpmSettings() rejects a scope declared without its @', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { scopes: ['foo'] },
    },
  })).toThrow(/should list "@"-prefixed scopes, but got "foo"/)
})

test('getOptionsFromPnpmSettings() rejects one scope routed to two registries', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { scopes: ['@foo'] },
      'https://artifactory.example/': { scopes: ['@foo'] },
    },
  })).toThrow(/The scope "@foo" is routed to two registries/)
})

test('getOptionsFromPnpmSettings() reads a declared prefix as a named registry', () => {
  // Keyed by the URL as written: a named registry's URL is what a lockfile's
  // recorded tarball URLs are matched against, so normalizing it here would
  // change what an existing lockfile verifies against.
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example': { prefix: 'work', serverType: 'artifactory' },
    },
  })
  expect(options.registriesByPrefix).toStrictEqual({ work: 'https://npm.corp.example' })
  expect(options.registryOptionsByUrl).toStrictEqual({
    'https://npm.corp.example/': { serverType: 'artifactory' },
  })
})

test('getOptionsFromPnpmSettings() rejects one prefix declared by two registries', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { prefix: 'work' },
      'https://artifactory.example/': { prefix: 'work' },
    },
  })).toThrow(/The prefix "work" is declared by two registries/)
})

test('getOptionsFromPnpmSettings() lets a declared prefix win over the deprecated namedRegistries', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    namedRegistries: {
      work: 'https://stale.example/',
      other: 'https://other.example/',
    },
    registries: {
      'https://npm.corp.example/': { prefix: 'work' },
    },
  })
  expect(options.registriesByPrefix).toStrictEqual({
    work: 'https://npm.corp.example/',
    other: 'https://other.example/',
  })
})

test('getOptionsFromPnpmSettings() records no options for a registry that only declares routes', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { scopes: ['@foo'] },
    },
  })
  expect(options.registryOptionsByUrl).toBeUndefined()
})

test('getOptionsFromPnpmSettings() rejects an unknown field in a registry declaration', () => {
  // A misspelled field would otherwise sit there doing nothing.
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { scope: '@acme' } as never,
    },
  })).toThrow(/is not a known registry setting/)
})

test('getOptionsFromPnpmSettings() reads a declared supportsTimeField', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { supportsTimeField: true },
      'https://artifactory.example/': { serverType: 'artifactory', supportsTimeField: false },
    },
  })
  expect(options.registryOptionsByUrl).toStrictEqual({
    'https://npm.corp.example/': { supportsTimeField: true },
    'https://artifactory.example/': { serverType: 'artifactory', supportsTimeField: false },
  })
})

test('getOptionsFromPnpmSettings() rejects a non-boolean supportsTimeField', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      'https://npm.corp.example/': { supportsTimeField: 'yes' as never },
    },
  })).toThrow(/supportsTimeField" setting should be a boolean, but got string/)
})

test('getOptionsFromPnpmSettings() translates virtualStoreType to enableGlobalVirtualStore', () => {
  for (const [virtualStoreType, expected] of [['global', true], ['project', false]] as const) {
    const options = getOptionsFromPnpmSettings(process.cwd(), { virtualStoreType }) as any // eslint-disable-line
    expect(options.enableGlobalVirtualStore).toBe(expected)
    expect(options.virtualStoreType).toBeUndefined()
  }
})

test('getOptionsFromPnpmSettings() lets virtualStoreType win over enableGlobalVirtualStore', () => {
  for (const [virtualStoreType, enableGlobalVirtualStore, expected] of [
    ['project', true, false],
    ['global', false, true],
  ] as const) {
    const options = getOptionsFromPnpmSettings(process.cwd(), {
      virtualStoreType,
      enableGlobalVirtualStore,
    }) as any // eslint-disable-line
    expect(options.enableGlobalVirtualStore).toBe(expected)
  }
})

test('getOptionsFromPnpmSettings() keeps enableGlobalVirtualStore working on its own', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    enableGlobalVirtualStore: true,
  }) as any // eslint-disable-line
  expect(options.enableGlobalVirtualStore).toBe(true)
})

test('getOptionsFromPnpmSettings() rejects an unknown virtualStoreType', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    virtualStoreType: 'shared' as never,
  })).toThrow(/The "virtualStoreType" setting should be one of global, project, but got "shared"/)
})
