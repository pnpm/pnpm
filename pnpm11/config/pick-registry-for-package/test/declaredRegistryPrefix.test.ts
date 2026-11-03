import { expect, test } from '@jest/globals'
import {
  isSameRegistry,
  normalizeRegistryKey,
  pickAliasKeyPrefix,
  pickDeclaredRegistryPrefix,
} from '@pnpm/config.pick-registry-for-package'

test('normalizeRegistryKey() compares the scheme and host case-insensitively and appends a trailing slash', () => {
  expect(normalizeRegistryKey('https://NPM.Example.com')).toBe('https://npm.example.com/')
  expect(normalizeRegistryKey('HTTPS://npm.example.com/')).toBe('https://npm.example.com/')
  expect(normalizeRegistryKey('https://npm.example.com/Team')).toBe('https://npm.example.com/Team/')
  expect(normalizeRegistryKey('https://npm.example.com:443/team/')).toBe('https://npm.example.com/team/')
  expect(normalizeRegistryKey('not a url')).toBeUndefined()
})

test('isSameRegistry() tells two spellings of one registry from two registries', () => {
  expect(isSameRegistry('https://X.example.com', 'https://x.example.com/')).toBe(true)
  expect(isSameRegistry('https://x.example.com/a', 'https://x.example.com/b/')).toBe(false)
  expect(isSameRegistry('https://x.example.com/', 'http://x.example.com/')).toBe(false)
  expect(isSameRegistry('not a url', 'not a url')).toBe(true)
})

const DEFAULT = 'https://registry.example.com/'

function registries (registriesByPrefix?: Record<string, string>, scopes: Record<string, string> = {}) {
  return { registriesByScope: { default: DEFAULT, ...scopes }, registriesByPrefix }
}

test('pickDeclaredRegistryPrefix() names the prefix that a registry declares', () => {
  const declared = registries({ work: 'https://NPM.example.com' })
  expect(pickDeclaredRegistryPrefix(declared, 'https://npm.example.com/')).toBe('work')
  expect(pickDeclaredRegistryPrefix(declared, 'https://other.example.com/')).toBeUndefined()
  expect(pickDeclaredRegistryPrefix(registries(), 'https://npm.example.com/')).toBeUndefined()
})

test('pickDeclaredRegistryPrefix() picks the lexicographically smallest of several prefixes of one registry', () => {
  expect(pickDeclaredRegistryPrefix(registries({
    zeta: 'https://npm.example.com/',
    work: 'https://npm.example.com',
  }), 'https://npm.example.com/')).toBe('work')
})

test('pickDeclaredRegistryPrefix() treats a declared built-in alias like any other prefix', () => {
  expect(pickDeclaredRegistryPrefix(registries({}), 'https://npm.pkg.github.com/')).toBeUndefined()
  expect(pickDeclaredRegistryPrefix(registries({ gh: 'https://npm.pkg.github.com/' }), 'https://npm.pkg.github.com/')).toBe('gh')
})

// https://github.com/pnpm/pnpm/issues/17575
test('pickDeclaredRegistryPrefix() never qualifies the keys of the default registry', () => {
  expect(pickDeclaredRegistryPrefix(registries({ npmjs: DEFAULT }), DEFAULT)).toBeUndefined()
  expect(pickDeclaredRegistryPrefix(registries({ work: 'https://REGISTRY.example.com' }), DEFAULT)).toBeUndefined()
})

test('pickDeclaredRegistryPrefix() ignores a prefix whose URL does not parse', () => {
  expect(pickDeclaredRegistryPrefix(registries({ broken: 'not a url' }), 'not a url')).toBeUndefined()
})

test('pickAliasKeyPrefix() keeps a plain key for a package of the default registry that a plain range routes there too', () => {
  const routed = registries(undefined, { '@corp': 'https://corp.example.com/' })
  expect(pickAliasKeyPrefix(routed, { alias: 'npmjs', name: 'foo', registry: DEFAULT })).toBeUndefined()
  expect(pickAliasKeyPrefix(routed, { alias: 'npmjs', name: '@corp/foo', registry: DEFAULT })).toBe('npmjs')
  expect(pickAliasKeyPrefix(routed, { alias: 'gh', name: 'foo', registry: 'https://npm.pkg.github.com/' })).toBe('gh')
})

test('pickAliasKeyPrefix() names the prefix the keys of the registry carry', () => {
  const declared = registries({ work: 'https://npm.example.com/', zeta: 'https://npm.example.com/' })
  expect(pickAliasKeyPrefix(declared, { alias: 'zeta', name: 'foo', registry: 'https://npm.example.com/' })).toBe('work')
})

test('pickAliasKeyPrefix() names the smallest prefix of the default registry for a package routed elsewhere', () => {
  const routed = registries({ zeta: DEFAULT, work: DEFAULT }, { '@corp': 'https://corp.example.com/' })
  expect(pickAliasKeyPrefix(routed, { alias: 'zeta', name: '@corp/foo', registry: DEFAULT })).toBe('work')
  expect(pickAliasKeyPrefix(routed, { alias: 'zeta', name: 'foo', registry: DEFAULT })).toBeUndefined()
})
