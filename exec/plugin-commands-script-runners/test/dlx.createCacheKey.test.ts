import { createHexHash } from '@pnpm/crypto.hash'
import { createCacheKey } from '../src/dlx'

test('creates a hash', () => {
  const received = createCacheKey(['shx', '@foo/bar'], {
    default: 'https://registry.npmjs.com/',
    '@foo': 'https://example.com/npm-registry/foo/',
  })
  const expected = createHexHash(JSON.stringify([['@foo/bar', 'shx'], [
    ['@foo', 'https://example.com/npm-registry/foo/'],
    ['default', 'https://registry.npmjs.com/'],
  ]]))
  expect(received).toBe(expected)
})

test('is agnostic to package order', () => {
  const registries = { default: 'https://registry.npmjs.com/' }
  expect(createCacheKey(['a', 'c', 'b'], registries)).toBe(createCacheKey(['a', 'b', 'c'], registries))
  expect(createCacheKey(['b', 'a', 'c'], registries)).toBe(createCacheKey(['a', 'b', 'c'], registries))
  expect(createCacheKey(['b', 'c', 'a'], registries)).toBe(createCacheKey(['a', 'b', 'c'], registries))
  expect(createCacheKey(['c', 'a', 'b'], registries)).toBe(createCacheKey(['a', 'b', 'c'], registries))
  expect(createCacheKey(['c', 'b', 'a'], registries)).toBe(createCacheKey(['a', 'b', 'c'], registries))
})

test('is agnostic to registry key order', () => {
  const packages = ['a', 'b', 'c']
  const foo = 'https://example.com/foo/'
  const bar = 'https://example.com/bar/'
  expect(createCacheKey(packages, { '@foo': foo, '@bar': bar })).toBe(createCacheKey(packages, { '@bar': bar, '@foo': foo }))
})
