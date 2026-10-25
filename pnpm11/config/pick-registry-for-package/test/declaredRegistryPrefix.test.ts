import { expect, test } from '@jest/globals'
import {
  isSameRegistry,
  normalizeRegistryKey,
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

test('pickDeclaredRegistryPrefix() names the prefix that a registry declares', () => {
  const declared = { work: 'https://NPM.example.com' }
  expect(pickDeclaredRegistryPrefix(declared, 'https://npm.example.com/')).toBe('work')
  expect(pickDeclaredRegistryPrefix(declared, 'https://other.example.com/')).toBeUndefined()
  expect(pickDeclaredRegistryPrefix(undefined, 'https://npm.example.com/')).toBeUndefined()
})

test('pickDeclaredRegistryPrefix() picks the lexicographically smallest of several prefixes of one registry', () => {
  expect(pickDeclaredRegistryPrefix({
    zeta: 'https://npm.example.com/',
    work: 'https://npm.example.com',
  }, 'https://npm.example.com/')).toBe('work')
})

test('pickDeclaredRegistryPrefix() does not treat the built-in aliases as declared', () => {
  expect(pickDeclaredRegistryPrefix({}, 'https://registry.npmjs.org/')).toBeUndefined()
  expect(pickDeclaredRegistryPrefix({ npmjs: 'https://registry.npmjs.org/' }, 'https://registry.npmjs.org/')).toBe('npmjs')
})

test('pickDeclaredRegistryPrefix() ignores a prefix whose URL does not parse', () => {
  expect(pickDeclaredRegistryPrefix({ broken: 'not a url' }, 'not a url')).toBeUndefined()
})
