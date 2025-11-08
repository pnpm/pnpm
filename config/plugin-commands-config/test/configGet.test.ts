import * as ini from 'ini'
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

  expect(getOutputString(getResult)).toEqual('~/store')
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

  expect(getOutputString(getResult)).toEqual('~/store')
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

  expect(getOutputString(getResult)).toEqual('true')
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

  expect(getOutputString(getResult)).toBe('*eslint*,*prettier*')
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

test('config get without key show list all settings ', async () => {
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
    // rawConfig keys are always kebab-case
    'package-extensions': {
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
    test.each([
      ['', rawConfig],
      ['packageExtensions', rawConfig['package-extensions']],
      ['packageExtensions["@babel/parser"]', rawConfig['package-extensions']['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', rawConfig['package-extensions']['@babel/parser'].peerDependencies],
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', rawConfig['package-extensions']['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"]', rawConfig['package-extensions']['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', rawConfig['package-extensions']['jest-circus'].dependencies],
      ['packageExtensions["jest-circus"].dependencies.slash', rawConfig['package-extensions']['jest-circus'].dependencies.slash],
    ] as Array<[string, unknown]>)('%s', async (propertyPath, expected) => {
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
      ['packageExtensions', rawConfig['package-extensions']],
      ['packageExtensions["@babel/parser"]', rawConfig['package-extensions']['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', rawConfig['package-extensions']['@babel/parser'].peerDependencies],
      ['packageExtensions["jest-circus"]', rawConfig['package-extensions']['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', rawConfig['package-extensions']['jest-circus'].dependencies],
    ] as Array<[string, unknown]>)('%s', async (propertyPath, expected) => {
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
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', rawConfig['package-extensions']['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"].dependencies.slash', rawConfig['package-extensions']['jest-circus'].dependencies.slash],
    ] as Array<[string, string]>)('%s', async (propertyPath, expected) => {
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
