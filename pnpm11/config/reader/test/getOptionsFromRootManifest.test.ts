import { afterEach, expect, test } from '@jest/globals'

import { getOptionsFromPnpmSettings } from '../lib/getOptionsFromRootManifest.js'

// Shallow-copy at module load: `process.env` is a mutable object, so a bare
// reference would capture subsequent test mutations and `afterEach` would
// "restore" from the polluted state.
const ORIGINAL_ENV = { ...process.env }

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

test('getOptionsFromPnpmSettings() rejects non-object resolutions values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: [] as unknown as Record<string, string>,
  } as any)).toThrow(expect.objectContaining({ // eslint-disable-line
    code: 'ERR_PNPM_INVALID_RESOLUTIONS',
    message: 'The resolutions field should be an object, but got array',
  }))
})

test('getOptionsFromPnpmSettings() keeps ${VAR} placeholders literal in resolutions values', () => {
  // `package.json` is repo-controlled, and `resolutions` flow into the
  // lockfile's `overrides` — a shared, persisted artifact. Expanding env
  // vars here would materialize victim environment secrets into the
  // lockfile. Users who need env expansion should move the override to
  // `pnpm-workspace.yaml`, which still expands env vars through
  // `replaceEnvInSettings`.
  //
  // Set the env var to a sentinel so the test fails loudly if the code
  // ever regresses to expanding (the assertion against the literal
  // placeholder would then receive "1.0.0" instead).
  process.env.PNPM_TEST_VERSION = '1.0.0'
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {
      foo: '${PNPM_TEST_VERSION}',
    },
  } as any) // eslint-disable-line
  expect(options.overrides).toStrictEqual({
    foo: '${PNPM_TEST_VERSION}',
  })
})

test('getOptionsFromPnpmSettings() ignores manifest resolutions when workspace overrides exist', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      baz: '3.0.0',
      bar: '2.5.0',
    },
  }, {
    resolutions: {
      foo: '1.0.0',
      bar: '2.0.0',
    },
  } as any) // eslint-disable-line
  expect(options.overrides).toStrictEqual({
    bar: '2.5.0',
    baz: '3.0.0',
  })
})

test('getOptionsFromPnpmSettings() uses manifest resolutions when no workspace overrides', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {
      foo: '1.0.0',
    },
  } as any) // eslint-disable-line
  expect(options.overrides).toStrictEqual({
    foo: '1.0.0',
  })
})

test('getOptionsFromPnpmSettings() uses workspace overrides when no manifest resolutions', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {
      bar: '2.5.0',
    },
  })
  expect(options.overrides).toStrictEqual({
    bar: '2.5.0',
  })
})

test('getOptionsFromPnpmSettings() uses manifest resolutions when workspace overrides is empty', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {},
  }, {
    resolutions: {
      foo: '1.0.0',
    },
  } as any) // eslint-disable-line
  expect(options.overrides).toStrictEqual({
    foo: '1.0.0',
  })
})

test('getOptionsFromPnpmSettings() produces no overrides when both are empty', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: {},
  }, {
    resolutions: {},
  } as any) // eslint-disable-line
  expect(options.overrides).toBeUndefined()
})

test('getOptionsFromPnpmSettings() produces no overrides when resolutions is empty', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {},
  } as any) // eslint-disable-line
  expect(options.overrides).toBeUndefined()
})

test('getOptionsFromPnpmSettings() sets resolutionsStatus.ignoredResolutions when overrides exist', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: { bar: '2.5.0' },
  }, {
    resolutions: { foo: '1.0.0' },
  } as any) // eslint-disable-line
  expect(options.resolutionsStatus).toStrictEqual({
    ignoredResolutions: true,
    usedResolutions: false,
  })
})

test('getOptionsFromPnpmSettings() sets resolutionsStatus.usedResolutions when no overrides', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: { foo: '1.0.0' },
  } as any) // eslint-disable-line
  expect(options.resolutionsStatus).toStrictEqual({
    ignoredResolutions: false,
    usedResolutions: true,
  })
})

test('getOptionsFromPnpmSettings() does not set resolutionsStatus when no resolutions', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    overrides: { bar: '2.5.0' },
  })
  expect(options.resolutionsStatus).toBeUndefined()
})

test('getOptionsFromPnpmSettings() does not set resolutionsStatus when resolutions is empty', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {},
  } as any) // eslint-disable-line
  expect(options.resolutionsStatus).toBeUndefined()
})

test('getOptionsFromPnpmSettings() rejects non-string resolutions values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {
      foo: null,
    } as unknown as Record<string, string>,
  } as any)).toThrow(expect.objectContaining({ // eslint-disable-line
    code: 'ERR_PNPM_INVALID_RESOLUTIONS',
    message: 'The value of resolutions.foo should be a string, but got null',
  }))
})

test('getOptionsFromPnpmSettings() rejects array resolutions values', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: {
      foo: [],
    } as unknown as Record<string, string>,
  } as any)).toThrow(expect.objectContaining({ // eslint-disable-line
    code: 'ERR_PNPM_INVALID_RESOLUTIONS',
    message: 'The value of resolutions.foo should be a string, but got array',
  }))
})

test('getOptionsFromPnpmSettings() rejects string resolutions field', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: 'bad',
  } as any)).toThrow(expect.objectContaining({ // eslint-disable-line
    code: 'ERR_PNPM_INVALID_RESOLUTIONS',
    message: 'The resolutions field should be an object, but got string',
  }))
})

test('getOptionsFromPnpmSettings() ignores null resolutions field', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: null,
  } as any) // eslint-disable-line
  expect(options.overrides).toBeUndefined()
  expect(options.resolutionsStatus).toBeUndefined()
})

test('getOptionsFromPnpmSettings() throws on unresolvable $version reference in resolutions', () => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {}, {
    resolutions: { bar: '$nonexistent' },
  } as any)).toThrow(expect.objectContaining({ // eslint-disable-line
    code: 'ERR_PNPM_CANNOT_RESOLVE_OVERRIDE_VERSION',
    message: 'Cannot resolve version $nonexistent in overrides. The direct dependencies don\'t have dependency "nonexistent".',
  }))
})

test('getOptionsFromPnpmSettings() resolves $version references in resolutions from manifest deps', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {}, {
    dependencies: { foo: '1.2.3' },
    resolutions: { bar: '$foo' },
  } as any) // eslint-disable-line
  expect(options.overrides).toStrictEqual({
    bar: '1.2.3',
  })
})
