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

describe('registryOverrides', () => {
  const registries = {
    default: 'https://registry.npmjs.org/',
    '@foo': 'https://registry.npmjs.org/',
  }

  test('per-package override beats scope registry', () => {
    const overrides = { '@foo/private-lib': 'https://npm.pkg.github.com/' }
    expect(pickRegistryForPackage(registries, '@foo/private-lib', undefined, overrides))
      .toBe('https://npm.pkg.github.com/')
  })

  test('unmatched package in overridden scope still uses scope registry', () => {
    const overrides = { '@foo/private-lib': 'https://npm.pkg.github.com/' }
    expect(pickRegistryForPackage(registries, '@foo/public-lib', undefined, overrides))
      .toBe('https://registry.npmjs.org/')
  })

  test('per-package override works for unscoped packages', () => {
    const overrides = { 'my-private-lib': 'https://private.example.com/' }
    expect(pickRegistryForPackage(registries, 'my-private-lib', undefined, overrides))
      .toBe('https://private.example.com/')
  })

  test('override matches real package name via npm: aliasing', () => {
    const overrides = { '@foo/private-lib': 'https://npm.pkg.github.com/' }
    expect(pickRegistryForPackage(registries, 'local-alias', 'npm:@foo/private-lib@1.0.0', overrides))
      .toBe('https://npm.pkg.github.com/')
  })

  test('falls through to scope/default when override map is empty or undefined', () => {
    expect(pickRegistryForPackage(registries, '@foo/private-lib', undefined, {}))
      .toBe('https://registry.npmjs.org/')
    expect(pickRegistryForPackage(registries, '@foo/private-lib'))
      .toBe('https://registry.npmjs.org/')
  })

  test('falls through when package name does not match override key', () => {
    const overrides = { '@foo/other': 'https://other.example.com/' }
    expect(pickRegistryForPackage(registries, '@foo/private-lib', undefined, overrides))
      .toBe('https://registry.npmjs.org/')
  })

  test('malformed npm: specifier does not match the empty-string override key', () => {
    // "npm:" with no package body must not accidentally look up overrides[''].
    const overrides = { '': 'https://should-not-be-used.example.com/' }
    expect(pickRegistryForPackage(registries, 'my-pkg', 'npm:', overrides))
      .toBe('https://registry.npmjs.org/')
  })

  test('npm: specifier with a leading @ but no version does not collapse to empty', () => {
    // "npm:@foo/pkg" (no @version) must resolve to the full name for override lookup.
    const overrides = { '@foo/pkg': 'https://override.example.com/' }
    expect(pickRegistryForPackage(registries, 'alias', 'npm:@foo/pkg', overrides))
      .toBe('https://override.example.com/')
  })

  test('override on unscoped package does not mask scope registry lookups', () => {
    const withScope = {
      default: 'https://registry.npmjs.org/',
      '@bar': 'https://bar.example.com/',
    }
    const overrides = { 'lodash': 'https://override.example.com/' }
    expect(pickRegistryForPackage(withScope, '@bar/thing', undefined, overrides))
      .toBe('https://bar.example.com/')
  })
})
