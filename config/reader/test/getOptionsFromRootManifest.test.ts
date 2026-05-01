import { afterEach, expect, test } from '@jest/globals'

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

test('getOptionsFromPnpmSettings() expands env variables inside registries values', () => {
  process.env.PNPM_TEST_TOKEN = 'secret'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    registries: {
      default: 'https://registry.npmjs.org/',
      '@scope': 'https://registry.example.com/${PNPM_TEST_TOKEN}/',
    },
  }) as any // eslint-disable-line
  expect(options.registries['@scope']).toBe('https://registry.example.com/secret/')
})

test('getOptionsFromPnpmSettings() expands env variables inside namedRegistries values', () => {
  process.env.PNPM_TEST_HOST = 'work.example.com'
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    namedRegistries: {
      work: 'https://${PNPM_TEST_HOST}/npm/',
    },
  } as any) as any // eslint-disable-line
  expect(options.namedRegistries.work).toBe('https://work.example.com/npm/')
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
