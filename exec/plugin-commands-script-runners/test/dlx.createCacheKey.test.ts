import { createHexHash } from '@pnpm/crypto.hash'
import { createCacheKey } from '../src/dlx'

test('creates a hash', () => {
  const received = createCacheKey(['shx', '@foo/bar'], {
    registries: {
      default: 'https://registry.npmjs.com/',
      '@foo': 'https://example.com/npm-registry/foo/',
    },
  })
  const expected = createHexHash(JSON.stringify([['@foo/bar', 'shx'], [
    ['@foo', 'https://example.com/npm-registry/foo/'],
    ['default', 'https://registry.npmjs.com/'],
  ]]))
  expect(received).toBe(expected)
})

test('is agnostic to package order', () => {
  const registries = { default: 'https://registry.npmjs.com/' }
  const opts = { registries }
  expect(createCacheKey(['a', 'c', 'b'], opts)).toBe(createCacheKey(['a', 'b', 'c'], opts))
  expect(createCacheKey(['b', 'a', 'c'], opts)).toBe(createCacheKey(['a', 'b', 'c'], opts))
  expect(createCacheKey(['b', 'c', 'a'], opts)).toBe(createCacheKey(['a', 'b', 'c'], opts))
  expect(createCacheKey(['c', 'a', 'b'], opts)).toBe(createCacheKey(['a', 'b', 'c'], opts))
  expect(createCacheKey(['c', 'b', 'a'], opts)).toBe(createCacheKey(['a', 'b', 'c'], opts))
})

test('is agnostic to registry key order', () => {
  const packages = ['a', 'b', 'c']
  const foo = 'https://example.com/foo/'
  const bar = 'https://example.com/bar/'
  expect(createCacheKey(packages, {
    registries: { '@foo': foo, '@bar': bar },
  })).toBe(createCacheKey(packages, {
    registries: { '@bar': bar, '@foo': foo },
  }))
})

test('is agnostic to supportedArchitectures values order', () => {
  const packages = ['a', 'b', 'c']
  const registries = { default: 'https://registry.npmjs.com/' }
  expect(createCacheKey(packages, {
    registries,
    supportedArchitectures: {
      os: ['win32', 'linux', 'darwin'],
      cpu: ['x86_64', 'armv7', 'i686'],
    },
  })).toBe(createCacheKey(packages, {
    registries,
    supportedArchitectures: {
      cpu: ['armv7', 'i686', 'x86_64'],
      os: ['darwin', 'linux', 'win32'],
    },
  }))
})
