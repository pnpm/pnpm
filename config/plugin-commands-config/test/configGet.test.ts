import * as ini from 'ini'
import { config } from '@pnpm/plugin-commands-config'

/**
 * Recursively clone an object and give every object inside the clone a null prototype.
 * Making it possible to compare it to the result of `ini.decode` with `toStrictEqual`.
 */
function deepNullProto<Value> (value: Value): Value {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value

  const result: Value = Object.create(null)
  for (const key in value) {
    result[key] = deepNullProto(value[key])
  }
  return result
}

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

  expect(typeof getResult === 'object' && 'output' in getResult && getResult.output).toEqual('~/store')
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

  expect(typeof getResult === 'object' && 'output' in getResult && getResult.output).toEqual('~/store')
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

  expect(typeof getResult === 'object' && 'output' in getResult && getResult.output).toEqual('true')
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

  expect(typeof getResult === 'object' && 'output' in getResult && getResult.output).toBe('*eslint*,*prettier*')
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

  expect(typeof getResult === 'object' && 'output' in getResult && ini.decode(getResult.output)).toStrictEqual(deepNullProto({ react: '^19.0.0' }))
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
  function getOutputString (result: config.ConfigHandlerResult): string {
    if (result == null) throw new Error('output is null or undefined')
    if (typeof result === 'string') return result
    if (typeof result === 'object') return result.output
    const _typeGuard: never = result // eslint-disable-line @typescript-eslint/no-unused-vars
    throw new Error('unreachable')
  }

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

      expect(ini.decode(getOutputString(getResult))).toStrictEqual(deepNullProto(expected))
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
