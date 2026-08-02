import { expect, test } from '@jest/globals'
import { createKnownRegistries } from '@pnpm/config.pick-registry-for-package'
import { BUILTIN_NAMED_REGISTRIES } from '@pnpm/constants'

/**
 * The reverse-routing prefix list decides which registry pnpm verifies a
 * lockfile entry against when that entry carries a recorded tarball URL —
 * including entries that name no alias. Adding a built-in alias therefore
 * redirects verification traffic for anyone whose lockfile records a URL
 * under it, whether or not they use the alias.
 *
 * This assertion exists so that consequence has to be acknowledged: a new
 * built-in cannot land without updating this list.
 */
test('the default reverse-routing prefixes are exactly the built-in registries', () => {
  expect(createKnownRegistries().tarballPrefixes).toStrictEqual([
    'https://npm.pkg.github.com/',
    'https://registry.npmjs.org/',
  ])
})

test('every built-in alias contributes a prefix', () => {
  const { tarballPrefixes } = createKnownRegistries()

  expect(tarballPrefixes).toHaveLength(Object.keys(BUILTIN_NAMED_REGISTRIES).length)
})

test('a user mapping replaces the built-in prefix it overrides', () => {
  const { tarballPrefixes } = createKnownRegistries({ npmjs: 'https://npm.internal.example/' })

  // The point of the override for a proxying org: nothing routes to the
  // public host any more.
  expect(tarballPrefixes).toContain('https://npm.internal.example/')
  expect(tarballPrefixes).not.toContain('https://registry.npmjs.org/')
})

test('prefixes are ordered longest first so the deepest match wins', () => {
  const { tarballPrefixes } = createKnownRegistries({
    team: 'https://npm.example/team',
    teamSub: 'https://npm.example/team/sub',
  })

  const lengths = tarballPrefixes.map((prefix) => prefix.length)
  expect(lengths).toStrictEqual([...lengths].sort((a, b) => b - a))
  expect(tarballPrefixes.indexOf('https://npm.example/team/sub/'))
    .toBeLessThan(tarballPrefixes.indexOf('https://npm.example/team/'))
})

/**
 * Without the trailing slash a lookalike host registered as
 * `npm.pkg.github.com-evil` would match the GitHub Packages prefix.
 */
test('every prefix ends in a slash', () => {
  const { tarballPrefixes } = createKnownRegistries({ noSlash: 'https://npm.example/team' })

  for (const prefix of tarballPrefixes) {
    expect(prefix.endsWith('/')).toBe(true)
  }
})

test('a malformed user URL is dropped rather than poisoning the prefix list', () => {
  const { tarballPrefixes } = createKnownRegistries({ broken: 'not a url' })

  expect(tarballPrefixes).toStrictEqual([...createKnownRegistries().tarballPrefixes])
})

test('alias lookup is prototype-free so a crafted alias cannot resolve', () => {
  const { byAlias } = createKnownRegistries()

  // A dep path of `foo@constructor:1.0.0` must not find a truthy value and
  // sail past the guards that fail closed on an unknown alias.
  expect(byAlias.constructor).toBeUndefined()
  expect(byAlias.toString).toBeUndefined()
})
