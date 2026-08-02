import { expect, test } from '@jest/globals'
import { normalizeNamedRegistries } from '@pnpm/config.normalize-registries'
import { namedRegistryTarballPrefixes } from '@pnpm/config.pick-registry-for-package'
import { BUILTIN_NAMED_REGISTRIES } from '@pnpm/constants'

// A new built-in redirects verification traffic; this makes that explicit.
test('the default reverse-routing prefixes are exactly the built-in registries', () => {
  expect(namedRegistryTarballPrefixes(normalizeNamedRegistries())).toStrictEqual([
    'https://npm.pkg.github.com/',
    'https://registry.npmjs.org/',
  ])
})

test('every built-in alias contributes a prefix', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries())

  expect(tarballPrefixes).toHaveLength(Object.keys(BUILTIN_NAMED_REGISTRIES).length)
})

test('a user mapping replaces the built-in prefix it overrides', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries({ npmjs: 'https://npm.internal.example/' }))

  expect(tarballPrefixes).toContain('https://npm.internal.example/')
  expect(tarballPrefixes).not.toContain('https://registry.npmjs.org/')
})

test('prefixes are ordered longest first so the deepest match wins', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries({
    team: 'https://npm.example/team',
    teamSub: 'https://npm.example/team/sub',
  }))

  const lengths = tarballPrefixes.map((prefix) => prefix.length)
  expect(lengths).toStrictEqual([...lengths].sort((a, b) => b - a))
  expect(tarballPrefixes.indexOf('https://npm.example/team/sub/'))
    .toBeLessThan(tarballPrefixes.indexOf('https://npm.example/team/'))
})

test('every prefix ends in a slash', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries({ noSlash: 'https://npm.example/team' }))

  for (const prefix of tarballPrefixes) {
    expect(prefix.endsWith('/')).toBe(true)
  }
})

test('a malformed user URL is dropped rather than poisoning the prefix list', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries({ broken: 'not a url' }))

  expect(tarballPrefixes).toStrictEqual([...namedRegistryTarballPrefixes(normalizeNamedRegistries())])
})

test('the normalized map is prototype-free so a crafted name cannot resolve', () => {
  const namedRegistries = normalizeNamedRegistries()

  expect(namedRegistries.constructor).toBeUndefined()
  expect(namedRegistries.toString).toBeUndefined()
})

test('equal-length prefixes are ordered lexicographically, not by hash order', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeNamedRegistries({
    b: 'https://npm.example/bbb/',
    a: 'https://npm.example/aaa/',
  }))

  const sameLength = tarballPrefixes.filter((prefix) => prefix.startsWith('https://npm.example/'))
  expect(sameLength).toStrictEqual(['https://npm.example/aaa/', 'https://npm.example/bbb/'])
})

test('one instance is shared per alias map, so per-package callers do not rebuild it', () => {
  const namedRegistries = normalizeNamedRegistries({ work: 'https://npm.enterprise.example/' })

  expect(namedRegistryTarballPrefixes(namedRegistries)).toBe(namedRegistryTarballPrefixes(namedRegistries))
  // The no-config case only hits the cache because the default is shared.
  expect(namedRegistryTarballPrefixes(normalizeNamedRegistries()))
    .toBe(namedRegistryTarballPrefixes(normalizeNamedRegistries()))
})
