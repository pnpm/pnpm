import { getObjectValueByPropertyPathString } from '../src/index.js'

const OBJECT = {
  packages: [
    'foo',
    'bar',
  ],

  catalogs: {
    default: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
  },

  packageExtensions: {
    '@babel/parser': {
      peerDependencies: {
        unified: '*',
      },
    },
  },

  updateConfig: {
    ignoreDependencies: [
      'boxen',
      'camelcase',
      'find-up',
    ],
  },
} as const

test('path exists', () => {
  expect(getObjectValueByPropertyPathString(OBJECT, '')).toBe(OBJECT)
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages')).toBe(OBJECT.packages)
  expect(getObjectValueByPropertyPathString(OBJECT, '.packages')).toBe(OBJECT.packages)
  expect(getObjectValueByPropertyPathString(OBJECT, '["packages"]')).toBe(OBJECT.packages)
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages[0]')).toBe(OBJECT.packages[0])
  expect(getObjectValueByPropertyPathString(OBJECT, '.packages[0]')).toBe(OBJECT.packages[0])
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages[1]')).toBe(OBJECT.packages[1])
  expect(getObjectValueByPropertyPathString(OBJECT, '.packages[1]')).toBe(OBJECT.packages[1])
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs')).toBe(OBJECT.catalogs)
  expect(getObjectValueByPropertyPathString(OBJECT, '.catalogs')).toBe(OBJECT.catalogs)
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.default')).toBe(OBJECT.catalogs.default)
  expect(getObjectValueByPropertyPathString(OBJECT, '.catalogs.default')).toBe(OBJECT.catalogs.default)
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.default["is-positive"]')).toBe(OBJECT.catalogs.default['is-positive'])
  expect(getObjectValueByPropertyPathString(OBJECT, '.catalogs.default["is-positive"]')).toBe(OBJECT.catalogs.default['is-positive'])
})

test('path does not exist', () => {
  expect(getObjectValueByPropertyPathString(OBJECT, 'notExist')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, '.notExist')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.notExist')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, '.notExist.catalogs')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.default.notExist')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, '.catalogs.notExist.default')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages[99]')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages[0].foo')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.default["not-exist"]')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'catalogs.default["is-positive"].foo')).toBeUndefined()
})

test('does not leak JavaScript-specific properties', () => {
  expect(getObjectValueByPropertyPathString({}, 'constructor')).toBeUndefined()
  expect(getObjectValueByPropertyPathString([], 'length')).toBeUndefined()
  expect(getObjectValueByPropertyPathString('foo', 'length')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(0, 'valueOf')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(class {}, 'prototype')).toBeUndefined() // eslint-disable-line @typescript-eslint/no-extraneous-class
  expect(getObjectValueByPropertyPathString(OBJECT, 'constructor')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages.length')).toBeUndefined()
  expect(getObjectValueByPropertyPathString(OBJECT, 'packages[0].length')).toBeUndefined()
})

test('non-objects', () => {
  expect(getObjectValueByPropertyPathString(0, '')).toBe(0)
  expect(getObjectValueByPropertyPathString('foo', '')).toBe('foo')
})

test('does not allow accessing specific character in a string', () => {
  expect(getObjectValueByPropertyPathString('foo', '[0]')).toBeUndefined()
  expect(getObjectValueByPropertyPathString('foo', '["0"]')).toBeUndefined()
})
