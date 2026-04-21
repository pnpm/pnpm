import { describe, expect, test } from '@jest/globals'
import { config } from '@pnpm/config.commands'

import { createConfigCommandOpts, getOutputString } from './utils/index.js'

test('config get', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    storeDir: '~/store',
  }), ['get', 'store-dir'])

  expect(getOutputString(getResult)).toBe('~/store')
})

test('config get works with camelCase', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    storeDir: '~/store',
  }), ['get', 'storeDir'])

  expect(getOutputString(getResult)).toBe('~/store')
})

test('config get a boolean should return string format', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    updateNotifier: true,
  }), ['get', 'update-notifier'])

  expect(getOutputString(getResult)).toBe('true')
})

test('config get on array should return a comma-separated list', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    publicHoistPattern: [
      '*eslint*',
      '*prettier*',
    ],
  }), ['get', 'public-hoist-pattern'])

  expect(JSON.parse(getOutputString(getResult))).toStrictEqual([
    '*eslint*',
    '*prettier*',
  ])
})

test('config get on object should return a JSON string', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    catalog: {
      react: '^19.0.0',
    },
  }), ['get', 'catalog'])

  expect(JSON.parse(getOutputString(getResult))).toStrictEqual({ react: '^19.0.0' })
})

test('config get without key show list all settings', async () => {
  const authConfig = {
    'store-dir': '~/store',
    'fetch-retries': '2',
  }
  const baseOpts = {
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    authConfig,
  }
  const getOutput = await config.handler(createConfigCommandOpts(baseOpts), ['get'])

  const listOutput = await config.handler(createConfigCommandOpts(baseOpts), ['list'])

  expect(getOutput).toStrictEqual(listOutput)
})

describe('config get with a property path', () => {
  const packageExtensions = {
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
  }
  const configData = {
    dlxCacheMaxAge: '1234',
    trustPolicyExclude: ['foo', 'bar'],
    packageExtensions,
  }
  const baseOpts = createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {},
    ...configData,
  })

  describe('anything with --json', () => {
    test('«»', async () => {
      const getResult = await config.handler({
        ...baseOpts,
        json: true,
      }, ['get', ''])

      expect(JSON.parse(getOutputString(getResult))).toMatchObject(configData)
    })

    test.each([
      ['dlx-cache-max-age', configData.dlxCacheMaxAge],
      ['dlxCacheMaxAge', configData.dlxCacheMaxAge],
      ['trust-policy-exclude', configData.trustPolicyExclude],
      ['trustPolicyExclude', configData.trustPolicyExclude],
      ['trustPolicyExclude[0]', configData.trustPolicyExclude[0]],
      ['trustPolicyExclude[1]', configData.trustPolicyExclude[1]],
      ['packageExtensions', configData.packageExtensions],
      ['packageExtensions["@babel/parser"]', configData.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', configData.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', configData.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"]', configData.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', configData.packageExtensions['jest-circus'].dependencies],
      ['packageExtensions["jest-circus"].dependencies.slash', configData.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler({
        ...baseOpts,
        json: true,
      }, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('object without --json', () => {
    // Note: empty path returns all config including dir/global/configDir,
    // so we use toMatchObject for the empty-path case.
    test('«»', async () => {
      const getResult = await config.handler(baseOpts, ['get', ''])
      expect(JSON.parse(getOutputString(getResult))).toMatchObject(configData)
    })

    test.each([
      ['packageExtensions', configData.packageExtensions],
      ['packageExtensions["@babel/parser"]', configData.packageExtensions['@babel/parser']],
      ['packageExtensions["@babel/parser"].peerDependencies', configData.packageExtensions['@babel/parser'].peerDependencies],
      ['packageExtensions["jest-circus"]', configData.packageExtensions['jest-circus']],
      ['packageExtensions["jest-circus"].dependencies', configData.packageExtensions['jest-circus'].dependencies],
    ] as Array<[string, unknown]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler(baseOpts, ['get', propertyPath])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(expected)
    })
  })

  describe('string without --json', () => {
    test.each([
      ['dlx-cache-max-age', configData.dlxCacheMaxAge],
      ['dlxCacheMaxAge', configData.dlxCacheMaxAge],
      ['trustPolicyExclude[0]', configData.trustPolicyExclude[0]],
      ['trustPolicyExclude[1]', configData.trustPolicyExclude[1]],
      ['packageExtensions["@babel/parser"].peerDependencies["@babel/types"]', configData.packageExtensions['@babel/parser'].peerDependencies['@babel/types']],
      ['packageExtensions["jest-circus"].dependencies.slash', configData.packageExtensions['jest-circus'].dependencies.slash],
    ] as Array<[string, string]>)('«%s»', async (propertyPath, expected) => {
      const getResult = await config.handler(baseOpts, ['get', propertyPath])

      expect(getOutputString(getResult)).toStrictEqual(expected)
    })
  })

  describe('non-rc kebab-case keys', () => {
    test('«package-extensions» resolves to packageExtensions on Config', async () => {
      const getResult = await config.handler(baseOpts, ['get', 'package-extensions'])

      expect(JSON.parse(getOutputString(getResult))).toStrictEqual(configData.packageExtensions)
    })

    test('unknown kebab-case key returns undefined', async () => {
      const getResult = await config.handler(baseOpts, ['get', 'no-such-setting'])

      expect(getOutputString(getResult)).toBe('undefined')
    })
  })
})

test('config get with scoped registry key (global: false)', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: false,
    authConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
  }), ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('https://custom-registry.example.com/')
})

test('config get with scoped registry key (global: true)', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: true,
    authConfig: {
      '@scope:registry': 'https://custom-registry.example.com/',
    },
  }), ['get', '@scope:registry'])

  expect(getOutputString(getResult)).toBe('https://custom-registry.example.com/')
})

test('config get with scoped registry key that does not exist', async () => {
  const getResult = await config.handler(createConfigCommandOpts({
    dir: process.cwd(),
    cliOptions: {},
    configDir: process.cwd(),
    global: false,
    authConfig: {},
  }), ['get', '@scope:registry'])

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
    const getResult = await config.handler(createConfigCommandOpts({
      dir: process.cwd(),
      cliOptions: {},
      configDir: process.cwd(),
      global: true,
      authConfig: {},
    }), ['get', key])

    expect(getOutputString(getResult)).toBe('undefined')
  })
})
