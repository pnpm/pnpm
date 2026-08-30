import { expect, test } from '@jest/globals'
import { toRegistryDeclarations } from '@pnpm/config.normalize-registries'

test('toRegistryDeclarations() gathers every route to a registry into its entry', () => {
  expect(toRegistryDeclarations({
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@acme': 'https://npm.corp.example/',
      '@acme-internal': 'https://npm.corp.example/',
      '@other': 'https://npm.other.example/',
    },
    registriesByPrefix: { work: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { serverType: 'artifactory' } },
  })).toStrictEqual({
    'https://npm.corp.example/': {
      scopes: ['@acme', '@acme-internal'],
      prefix: 'work',
      serverType: 'artifactory',
    },
    'https://npm.other.example/': { scopes: ['@other'] },
  })
})

test('toRegistryDeclarations() leaves the default registry to the request that carries it', () => {
  expect(toRegistryDeclarations({
    registriesByScope: { default: 'https://npm.corp.example/' },
  })).toStrictEqual({})
})

test('toRegistryDeclarations() declares a registry that only has a layout', () => {
  // The default registry's layout reaches the server this way: its URL is a
  // key here even though its route travels as the request's `registry`.
  expect(toRegistryDeclarations({
    registriesByScope: { default: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { serverType: 'npm' } },
  })).toStrictEqual({
    'https://npm.corp.example/': { serverType: 'npm' },
  })
})

test('toRegistryDeclarations() keys a prefix by the URL as written', () => {
  // A named registry's URL is what a lockfile's recorded tarball URLs are
  // matched against, so it must not pick up a normalized trailing slash.
  expect(toRegistryDeclarations({
    registriesByPrefix: { work: 'https://npm.corp.example' },
  })).toStrictEqual({
    'https://npm.corp.example': { prefix: 'work' },
  })
})

test('toRegistryDeclarations() does not declare a built-in route the user did not point elsewhere', () => {
  // Declaring `@jsr` would put npm.jsr.io in front of a pnpr server's
  // allowlist on every request, including those that resolve no JSR package.
  expect(toRegistryDeclarations({
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@jsr': 'https://npm.jsr.io/',
    },
  })).toStrictEqual({})

  expect(toRegistryDeclarations({
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@jsr': 'https://jsr.corp.example/',
    },
  })).toStrictEqual({
    'https://jsr.corp.example/': { scopes: ['@jsr'] },
  })
})

test('toRegistryDeclarations() carries every fact a registry declares', () => {
  expect(toRegistryDeclarations({
    registryOptionsByUrl: {
      'https://npm.corp.example/': { serverType: 'artifactory', supportsTimeField: true },
      'https://time.example/': { supportsTimeField: false },
    },
  })).toStrictEqual({
    'https://npm.corp.example/': { serverType: 'artifactory', supportsTimeField: true },
    'https://time.example/': { supportsTimeField: false },
  })
})
