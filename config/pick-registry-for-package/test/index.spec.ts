import { expect, test } from '@jest/globals'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'

test('pick correct scope', () => {
  const registries = {
    default: 'https://registry.npmjs.org/',
    '@private': 'https://private.registry.com/',
  }
  expect(pickRegistryForPackage(registries, '@private/lodash')).toBe('https://private.registry.com/')
  expect(pickRegistryForPackage(registries, '@random/lodash')).toBe('https://registry.npmjs.org/')
  expect(pickRegistryForPackage(registries, '@random/lodash', 'npm:@private/lodash@1')).toBe('https://private.registry.com/')
})

// An unscoped `npm:` alias target (e.g. `"@private/foo": "npm:lodash@^1"`)
// must NOT route through the local alias's scope: the fetched package is
// `lodash` (unscoped) and doesn't live on the `@private` registry. The npm-
// alias branch returns `null` in that case so the call falls through to
// `registries.default`.
test('unscoped npm-alias target routes to default, not the local alias scope', () => {
  const registries = {
    default: 'https://registry.npmjs.org/',
    '@private': 'https://private.registry.com/',
  }
  expect(pickRegistryForPackage(registries, '@private/foo', 'npm:lodash@^1')).toBe('https://registry.npmjs.org/')
})

// Scoped local + scoped `npm:` target in a different scope: the target's
// scope wins. The package being fetched is `@scope2/bar`, so routing
// follows `@scope2`, not the local `@scope1/` slot.
test('scoped npm-alias target in different scope wins over local scope', () => {
  const registries = {
    default: 'https://registry.npmjs.org/',
    '@scope1': 'https://scope1.registry/',
    '@scope2': 'https://scope2.registry/',
  }
  expect(pickRegistryForPackage(registries, '@scope1/foo', 'npm:@scope2/bar@^1')).toBe('https://scope2.registry/')
})
