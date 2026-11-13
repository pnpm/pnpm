import { expect, test } from '@jest/globals'
import { createShortHash } from '@pnpm/crypto.hash'
import { engineName } from '@pnpm/engine.runtime.system-version'

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
  ], { engine: engineName() }]))
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

test('depends on the Node.js major version', () => {
  const opts = {
    packages: ['a'],
    registriesByScope: { default: 'https://registry.npmjs.com/' },
  }
  expect(createCacheKey({ ...opts, nodeVersion: '22.1.0' })).toBe(createCacheKey({ ...opts, nodeVersion: 'v22.9.0' }))
  expect(createCacheKey({ ...opts, nodeVersion: '22.1.0' })).not.toBe(createCacheKey({ ...opts, nodeVersion: '24.1.0' }))
})

test('distinguishes and normalizes allowBuild negations', () => {
  const base = {
    packages: ['demo-pkg'],
    registriesByScope: { default: 'https://registry.npmjs.com/' },
  }
  const keyDefault = createCacheKey(base)
  const keyAllowed = createCacheKey({ ...base, allowBuild: ['demo-pkg'] })
  const keyDenied = createCacheKey({ ...base, allowBuild: ['!demo-pkg'] })
  expect(keyAllowed).not.toBe(keyDefault)
  expect(keyDenied).not.toBe(keyDefault)
  expect(keyDenied).not.toBe(keyAllowed)
  expect(createCacheKey({ ...base, allowBuild: ['demo-pkg', '!demo-pkg'] })).toBe(keyDenied)
  expect(createCacheKey({ ...base, allowBuild: ['!demo-pkg', 'demo-pkg'] })).toBe(keyAllowed)
})

test('changes with active overrides, packageExtensions, and security policies while keeping inactive defaults stable', () => {
  const base = {
    packages: ['a@1.0.0'],
    registriesByScope: { default: 'https://registry.npmjs.com/' },
  }
  const defaultKey = createCacheKey(base)
  expect(createCacheKey({
    ...base,
    overrides: {},
    packageExtensions: {},
    minimumReleaseAge: 0,
    minimumReleaseAgeStrict: false,
    trustPolicy: 'off',
    trustLockfile: false,
    blockExoticSubdeps: true,
  })).toBe(defaultKey)

  const overridesForward = createCacheKey({ ...base, overrides: { 'alpha@^1.0.0': '1.0.0', 'alpha@>=1.0.0': '1.2.0' } })
  const overridesReversed = createCacheKey({ ...base, overrides: { 'alpha@>=1.0.0': '1.2.0', 'alpha@^1.0.0': '1.0.0' } })
  expect(overridesForward).not.toBe(defaultKey)
  expect(overridesForward).not.toBe(overridesReversed)

  const extensionsForward = createCacheKey({
    ...base,
    packageExtensions: {
      alpha: { dependencies: { b: '^2.0.0', a: '^1.0.0' } },
      'alpha@^1.0.0': { dependencies: { a: '^2.0.0' } },
    },
  })
  const extensionsInnerReordered = createCacheKey({
    ...base,
    packageExtensions: {
      alpha: { dependencies: { a: '^1.0.0', b: '^2.0.0' } },
      'alpha@^1.0.0': { dependencies: { a: '^2.0.0' } },
    },
  })
  const extensionsSelectorsReversed = createCacheKey({
    ...base,
    packageExtensions: {
      'alpha@^1.0.0': { dependencies: { a: '^2.0.0' } },
      alpha: { dependencies: { a: '^1.0.0', b: '^2.0.0' } },
    },
  })
  const extensionsChanged = createCacheKey({
    ...base,
    packageExtensions: {
      alpha: { dependencies: { a: '^1.0.0', b: '^3.0.0' } },
      'alpha@^1.0.0': { dependencies: { a: '^2.0.0' } },
    },
  })
  expect(extensionsForward).not.toBe(defaultKey)
  expect(extensionsForward).toBe(extensionsInnerReordered)
  expect(extensionsForward).not.toBe(extensionsSelectorsReversed)
  expect(extensionsForward).not.toBe(extensionsChanged)

  const ageStrict = createCacheKey({ ...base, minimumReleaseAge: 1440, minimumReleaseAgeStrict: true })
  const ageNonStrict = createCacheKey({ ...base, minimumReleaseAge: 1440, minimumReleaseAgeStrict: false })
  const ageDefaultLoose = createCacheKey({ ...base, minimumReleaseAge: 1440 })
  const ageExcluded = createCacheKey({ ...base, minimumReleaseAge: 1440, minimumReleaseAgeExclude: ['b', 'a'] })
  expect(ageStrict).not.toBe(defaultKey)
  expect(ageStrict).not.toBe(ageNonStrict)
  expect(ageDefaultLoose).toBe(ageNonStrict)
  expect(ageExcluded).not.toBe(ageNonStrict)
  expect(ageExcluded).toBe(createCacheKey({ ...base, minimumReleaseAge: 1440, minimumReleaseAgeExclude: ['a', 'b'] }))

  const trust = createCacheKey({ ...base, trustPolicy: 'no-downgrade' })
  const trustIgnoreAfter = createCacheKey({ ...base, trustPolicy: 'no-downgrade', trustPolicyIgnoreAfter: 60 })
  expect(trust).not.toBe(defaultKey)
  expect(trustIgnoreAfter).not.toBe(trust)
  expect(createCacheKey({ ...base, trustLockfile: true })).not.toBe(defaultKey)
  expect(createCacheKey({ ...base, blockExoticSubdeps: false })).not.toBe(defaultKey)
})
