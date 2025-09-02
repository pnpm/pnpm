import * as ini from 'ini'
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

test('config get on object should return an ini string', async () => {
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

  expect(ini.decode(getOutputString(getResult))).toEqual({ react: '^19.0.0' })
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

  expect(getOutput).toEqual(listOutput)
})

describe('config get with a property path', () => {
  const rawConfig = {
    'dlx-cache-max-age': '1234',
    'only-built-dependencies': ['foo', 'bar'],
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
        onlyBuiltDependencies: rawConfig['only-built-dependencies'],
        packageExtensions: rawConfig.packageExtensions,
      })
    })

    test.each([
      ['dlx-cache-max-age', rawConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', rawConfig['dlx-cache-max-age']],
      ['only-built-dependencies', rawConfig['only-built-dependencies']],
      ['onlyBuiltDependencies', rawConfig['only-built-dependencies']],
      ['onlyBuiltDependencies[0]', rawConfig['only-built-dependencies'][0]],
      ['onlyBuiltDependencies[1]', rawConfig['only-built-dependencies'][1]],
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
      ['', rawConfig],
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

      expect(ini.decode(getOutputString(getResult))).toEqual(expected)
    })
  })

  describe('string without --json', () => {
    test.each([
      ['dlx-cache-max-age', rawConfig['dlx-cache-max-age']],
      ['dlxCacheMaxAge', rawConfig['dlx-cache-max-age']],
      ['onlyBuiltDependencies[0]', rawConfig['only-built-dependencies'][0]],
      ['onlyBuiltDependencies[1]', rawConfig['only-built-dependencies'][1]],
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
    test.each(['package-extensions'])('«%s»', async (key) => {
      const getResult = await config.handler({
        dir: process.cwd(),
        cliOptions: {},
        configDir: process.cwd(),
        global: true,
        rawConfig,
      }, ['get', key])

      expect(getOutputString(getResult)).toBe('undefined')
    })
  })
})
