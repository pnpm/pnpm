import { config } from '@pnpm/config.commands'

import { getOutputString } from './utils/index.js'

test('config get', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      'store-dir': '~/store',
    },
    effectiveConfig: {
      'store-dir': '~/store',
    },
  }, ['get', 'store-dir'])

  expect(getOutputString(getResult)).toBe('~/store')
})

test('config get works with camelCase', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      'store-dir': '~/store',
    },
    effectiveConfig: {
      'store-dir': '~/store',
    },
  }, ['get', 'storeDir'])

  expect(getOutputString(getResult)).toBe('~/store')
})

test('config get a boolean should return string format', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      'update-notifier': true,
    },
    effectiveConfig: {
      'update-notifier': true,
    },
  }, ['get', 'update-notifier'])

  expect(getOutputString(getResult)).toBe('true')
})

test('config get on array should return a comma-separated list', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      'public-hoist-pattern': [
        '*eslint*',
        '*prettier*',
      ],
    },
    effectiveConfig: {
      'public-hoist-pattern': [
        '*eslint*',
        '*prettier*',
      ],
    },
  }, ['get', 'public-hoist-pattern'])

  expect(JSON.parse(getOutputString(getResult))).toStrictEqual([
    '*eslint*',
    '*prettier*',
  ])
})

test('config get on object should return a JSON string', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      catalog: {
        react: '^19.0.0',
      },
    },
    effectiveConfig: {
      catalog: {
        react: '^19.0.0',
      },
    },
  }, ['get', 'catalog'])

  expect(JSON.parse(getOutputString(getResult))).toStrictEqual({ react: '^19.0.0' })
})

test('config get without key show list all settings', async () => {
  const authConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
  }
  const getOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig,
    effectiveConfig: authConfig,
  }, ['get'])

  const listOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    authConfig,
    effectiveConfig: authConfig,
  }, ['list'])

  expect(getOutput).toStrictEqual(listOutput)
})

describe('config get with a property path', () => {
  // TODO: change `authConfig` into camelCase (to emulate pnpm-workspace.yaml)
  const authConfig = {
    'dlx-cache-max-age': '1234',
    'trust-policy-exclude': ['foo', 'bar'],
    packageExtensions: {
      '@babel/parser': {
        peerDependencies: {
          '@babel/types': '*',
        },
      },
      'jest-circus': {
        dependencies: {
          slash: '3',
        },
      },
    },
  }

  describe('anything with --json', () => {
    test('«»', async () => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        json: true,
        authConfig,
        effectiveConfig: authConfig,
      }, ['get', ''])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual({
        dlxCacheMaxAge: authConfig['dlx-cache-max-age'],
        trustPolicyExclude: authConfig['trust-policy-exclude'],
        packageExtensions: authConfig.packageExtensions,
      })
    })

    test.each([
      ['dlx-cache-max-age', authConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', authConfig['dlx-cache-max-age']],
      ['trust-policy-exclude', authConfig['trust-policy-exclude']],
      ['trustPolicyExclude', authConfig['trust-policy-exclude']],
      ['trustPolicyExclude[0]', authConfig['trust-policy-exclude'][0]],
      ['trustPolicyExclude[1]', authConfig['trust-policy-exclude'][1]],
      ['packageExtensions', authConfig.packageExtensions],
      ['packageExtensions["@babel/parser"]', authConfig.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', authConfig.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', authConfig.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"]', authConfig.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', authConfig.packageExtensions['jest-circus'].dependencies],
      ['packageExtensions["jest-circus"].dependencies.slash', authConfig.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        json: true,
        authConfig,
        effectiveConfig: authConfig,
      }, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('object without --json', () => {
    test.each([
      // TODO: change `authConfig` into camelCase and replace this object with just `authConfig`.
      ['', {
        dlxCacheMaxAge: authConfig['dlx-cache-max-age'],
        trustPolicyExclude: authConfig['trust-policy-exclude'],
        packageExtensions: authConfig.packageExtensions,
      }],

      ['packageExtensions', authConfig.packageExtensions],
      ['packageExtensions["@babel/parser"]', authConfig.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', authConfig.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["jest-circus"]', authConfig.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', authConfig.packageExtensions['jest-circus'].dependencies],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        authConfig,
        effectiveConfig: authConfig,
      }, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('string without --json', () => {
    test.each([
      ['dlx-cache-max-age', authConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', authConfig['dlx-cache-max-age']],
      ['trustPolicyExclude[0]', authConfig['trust-policy-exclude'][0]],
      ['trustPolicyExclude[1]', authConfig['trust-policy-exclude'][1]],
      ['package-extensions', 'undefined'], // it cannot be defined by rc, it can't be kebab-case
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', authConfig.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"].dependencies.slash', authConfig.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, string]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        authConfig,
        effectiveConfig: authConfig,
      }, ['get', propertyPath])

      expect(getOutputString(getResult)).toStrictEqual(expected)
    })
  })

  describe('non-rc kebab-case keys', () => {
    test('«package-extensions»', async () => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        authConfig,
        effectiveConfig: authConfig,
      }, ['get', 'package-extensions'])

      expect(getOutputString(getResult)).toBe('undefined')
    })
  })
})

test('config get with scoped registry key (global: false)', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: false,
    authConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
    effectiveConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
  }, ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('https://custom-registry.example.com/')
})

test('config get with scoped registry key (global: true)', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
    effectiveConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
  }, ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('https://custom-registry.example.com/')
})

test('config get with scoped registry key that does not exist', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: false,
    authConfig: {},
    effectiveConfig: {},
  }, ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('undefined')
})

// globalconfig and npm-globalconfig tests removed — pnpm no longer exposes these npm-compat properties

describe('does not traverse the prototype chain (#10296)', () => {
  test.each([
    'constructor',
    'hasOwnProperty',
    'isPrototypeOf',
    'toString',
    'valueOf',
    '__proto__',
  ])('%s', async key => {
    const getResult = await config.handler({
      dir: process.cwd(),
      cliOptions: {},
      configDir: process.cwd(),
      global: true,
      authConfig: {},
      effectiveConfig: {},
    }, ['get', key])

    expect(getOutputString(getResult)).toBe('undefined')
  })
})
