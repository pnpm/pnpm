import { expect, test } from '@jest/globals'
import { createShortHash } from '@pnpm/crypto.hash'

import { createCacheKey } from '../src/dlx.js'

test('creates a hash', () => {
  const received = createCacheKey({
    packages: ['shx', '@foo/bar'],
    registriesByScope: {
      default: 'https://registry.npmjs.com/',
      '@foo': 'https://example.com/npm-registry/foo/',
    },
  })
  const expected = createShortHash(JSON.stringify([['@foo/bar', 'shx'], [
    ['@foo', 'https://example.com/npm-registry/foo/'],
    ['default', 'https://registry.npmjs.com/'],
  ]]))
  expect(received).toBe(expected)
})

test('is agnostic to package order', () => {
  const registriesByScope = { default: 'https://registry.npmjs.com/' }
  const makeOpts = (packages: string[]) => ({ packages, registriesByScope })
  expect(createCacheKey(makeOpts(['a', 'c', 'b']))).toBe(createCacheKey(makeOpts(['a', 'b', 'c'])))
  expect(createCacheKey(makeOpts(['b', 'a', 'c']))).toBe(createCacheKey(makeOpts(['a', 'b', 'c'])))
  expect(createCacheKey(makeOpts(['b', 'c', 'a']))).toBe(createCacheKey(makeOpts(['a', 'b', 'c'])))
  expect(createCacheKey(makeOpts(['c', 'a', 'b']))).toBe(createCacheKey(makeOpts(['a', 'b', 'c'])))
  expect(createCacheKey(makeOpts(['c', 'b', 'a']))).toBe(createCacheKey(makeOpts(['a', 'b', 'c'])))
})

test('is agnostic to registry key order', () => {
  const packages = ['a', 'b', 'c']
  const foo = 'https://example.com/foo/'
  const bar = 'https://example.com/bar/'
  expect(createCacheKey({
    packages,
    registriesByScope: { '@foo': foo, '@bar': bar },
  })).toBe(createCacheKey({
    packages,
    registriesByScope: { '@bar': bar, '@foo': foo },
  }))
})

test('is agnostic to supportedArchitectures values order', () => {
  const packages = ['a', 'b', 'c']
  const registriesByScope = { default: 'https://registry.npmjs.com/' }
  expect(createCacheKey({
    packages,
    registriesByScope,
    supportedArchitectures: {
      os: ['win32', 'linux', 'darwin'],
      cpu: ['x86_64', 'armv7', 'i686'],
    },
  })).toBe(createCacheKey({
    packages,
    registriesByScope,
    supportedArchitectures: {
      cpu: ['armv7', 'i686', 'x86_64'],
      os: ['darwin', 'linux', 'win32'],
    },
  }))
})
