import { expect, test } from '@jest/globals'
import { normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import { namedRegistryTarballPrefixes } from '@pnpm/config.pick-registry-for-package'
import { BUILTIN_REGISTRIES_BY_PREFIX } from '@pnpm/constants'

// A new built-in redirects verification traffic; this makes that explicit.
test('the default reverse-routing prefixes are exactly the built-in registries', () => {
  expect(namedRegistryTarballPrefixes(normalizeRegistriesByPrefix())).toStrictEqual([
    'https://npm.pkg.github.com/',
    'https://registry.npmjs.org/',
  ])
})

test('every built-in alias contributes a prefix', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix())

  expect(tarballPrefixes).toHaveLength(Object.keys(BUILTIN_REGISTRIES_BY_PREFIX).length)
})

test('a user mapping replaces the built-in prefix it overrides', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix({ npmjs: 'https://npm.internal.example/' }))

  expect(tarballPrefixes).toContain('https://npm.internal.example/')
  expect(tarballPrefixes).not.toContain('https://registry.npmjs.org/')
})

test('prefixes are ordered longest first so the deepest match wins', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix({
    team: 'https://npm.example/team',
    teamSub: 'https://npm.example/team/sub',
  }))

  const lengths = tarballPrefixes.map((prefix) => prefix.length)
  expect(lengths).toStrictEqual([...lengths].sort((a, b) => b - a))
  expect(tarballPrefixes.indexOf('https://npm.example/team/sub/'))
    .toBeLessThan(tarballPrefixes.indexOf('https://npm.example/team/'))
})

test('every prefix ends in a slash', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix({ noSlash: 'https://npm.example/team' }))

  for (const prefix of tarballPrefixes) {
    expect(prefix.endsWith('/')).toBe(true)
  }
})

test('a malformed user URL is dropped rather than poisoning the prefix list', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix({ broken: 'not a url' }))

  expect(tarballPrefixes).toStrictEqual([...namedRegistryTarballPrefixes(normalizeRegistriesByPrefix())])
})

test('the normalized map is prototype-free so a crafted name cannot resolve', () => {
  const registriesByPrefix = normalizeRegistriesByPrefix()

  expect(registriesByPrefix.constructor).toBeUndefined()
  expect(registriesByPrefix.toString).toBeUndefined()
})

test('equal-length prefixes are ordered lexicographically, not by hash order', () => {
  const tarballPrefixes = namedRegistryTarballPrefixes(normalizeRegistriesByPrefix({
    b: 'https://npm.example/bbb/',
    a: 'https://npm.example/aaa/',
  }))

  const sameLength = tarballPrefixes.filter((prefix) => prefix.startsWith('https://npm.example/'))
  expect(sameLength).toStrictEqual(['https://npm.example/aaa/', 'https://npm.example/bbb/'])
})

test('one instance is shared per alias map, so per-package callers do not rebuild it', () => {
  const registriesByPrefix = normalizeRegistriesByPrefix({ work: 'https://npm.enterprise.example/' })

  expect(namedRegistryTarballPrefixes(registriesByPrefix)).toBe(namedRegistryTarballPrefixes(registriesByPrefix))
  // The no-config case only hits the cache because the default is shared.
  expect(namedRegistryTarballPrefixes(normalizeRegistriesByPrefix()))
    .toBe(namedRegistryTarballPrefixes(normalizeRegistriesByPrefix()))
})
