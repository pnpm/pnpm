import path from 'path'
import { config } from '@pnpm/plugin-commands-config'
import { getOutputString } from './utils/index.js'

test('config get', async () => {
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig: {
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
    rawConfig: {
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
    rawConfig: {
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
    rawConfig: {
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
    rawConfig: {
      catalog: {
        react: '^19.0.0',
      },
    },
  }, ['get', 'catalog'])

  expect(JSON.parse(getOutputString(getResult))).toStrictEqual({ react: '^19.0.0' })
})

test('config get without key show list all settings', async () => {
  const rawConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
  }
  const getOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig,
  }, ['get'])

  const listOutput = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    rawConfig,
  }, ['list'])

  expect(getOutput).toStrictEqual(listOutput)
})

describe('config get with a property path', () => {
  // TODO: change `rawConfig` into camelCase (to emulate pnpm-workspace.yaml)
  const rawConfig = {
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
        rawConfig,
      }, ['get', ''])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual({
        dlxCacheMaxAge: rawConfig['dlx-cache-max-age'],
        trustPolicyExclude: rawConfig['trust-policy-exclude'],
        packageExtensions: rawConfig.packageExtensions,
      })
    })

    test.each([
      ['dlx-cache-max-age', rawConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', rawConfig['dlx-cache-max-age']],
      ['trust-policy-exclude', rawConfig['trust-policy-exclude']],
      ['trustPolicyExclude', rawConfig['trust-policy-exclude']],
      ['trustPolicyExclude[0]', rawConfig['trust-policy-exclude'][0]],
      ['trustPolicyExclude[1]', rawConfig['trust-policy-exclude'][1]],
      ['packageExtensions', rawConfig.packageExtensions],
      ['packageExtensions["@babel/parser"]', rawConfig.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', rawConfig.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', rawConfig.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"]', rawConfig.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', rawConfig.packageExtensions['jest-circus'].dependencies],
      ['packageExtensions["jest-circus"].dependencies.slash', rawConfig.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        json: true,
        rawConfig,
      }, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('object without --json', () => {
    test.each([
      // TODO: change `rawConfig` into camelCase and replace this object with just `rawConfig`.
      ['', {
        dlxCacheMaxAge: rawConfig['dlx-cache-max-age'],
        trustPolicyExclude: rawConfig['trust-policy-exclude'],
        packageExtensions: rawConfig.packageExtensions,
      }],

      ['packageExtensions', rawConfig.packageExtensions],
      ['packageExtensions["@babel/parser"]', rawConfig.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', rawConfig.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["jest-circus"]', rawConfig.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', rawConfig.packageExtensions['jest-circus'].dependencies],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        rawConfig,
      }, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('string without --json', () => {
    test.each([
      ['dlx-cache-max-age', rawConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', rawConfig['dlx-cache-max-age']],
      ['trustPolicyExclude[0]', rawConfig['trust-policy-exclude'][0]],
      ['trustPolicyExclude[1]', rawConfig['trust-policy-exclude'][1]],
      ['package-extensions', 'undefined'], // it cannot be defined by rc, it can't be kebab-case
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', rawConfig.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"].dependencies.slash', rawConfig.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, string]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        rawConfig,
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
        rawConfig,
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
    rawConfig: {
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
    rawConfig: {
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
    rawConfig: {},
  }, ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('undefined')
})

test('config get globalconfig', async () => {
  const configDir = process.cwd()
  const expectedGlobalconfigPath = path.join(configDir, 'rc')
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir,
    global: true,
    rawConfig: {
      globalconfig: expectedGlobalconfigPath,
    },
  }, ['get', 'globalconfig'])

  expect(getOutputString(getResult)).toBe(expectedGlobalconfigPath)
})

test('config get npm-globalconfig', async () => {
  const npmGlobalconfigPath = path.join('/root', '.npmrc')
  const getResult = await config.handler({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    rawConfig: {
      'npm-globalconfig': npmGlobalconfigPath,
    },
  }, ['get', 'npm-globalconfig'])

  expect(getOutputString(getResult)).toBe(npmGlobalconfigPath)
})

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
      rawConfig: {},
    }, ['get', key])

    expect(getOutputString(getResult)).toBe('undefined')
  })
})
