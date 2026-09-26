import { expect, test } from '@jest/globals'
import { toResolvedRegistryDeclarations } from '@pnpm/config.normalize-registries'

test('toResolvedRegistryDeclarations() declares the built-in routes on a default setup', () => {
  expect(toResolvedRegistryDeclarations({})).toStrictEqual({
    'https://npm.jsr.io/': { scopes: ['@jsr'] },
    'https://npm.pkg.github.com/': { prefix: 'gh' },
    'https://registry.npmjs.org/': { scopes: ['@'], prefix: 'npmjs' },
  })
})

test('toResolvedRegistryDeclarations() declares the default registry as the bare @ scope', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByScope: { default: 'https://npm.corp.example/' },
  })).toStrictEqual({
    'https://npm.corp.example/': { scopes: ['@'] },
    'https://npm.jsr.io/': { scopes: ['@jsr'] },
    'https://npm.pkg.github.com/': { prefix: 'gh' },
    'https://registry.npmjs.org/': { prefix: 'npmjs' },
  })
})

test('toResolvedRegistryDeclarations() merges every route into the registry entry it belongs to', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByScope: {
      default: 'https://npm.corp.example/',
      '@jsr': 'https://jsr.corp.example/',
      '@acme': 'https://npm.corp.example/',
    },
    registriesByPrefix: { work: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { serverType: 'artifactory' } },
  })).toStrictEqual({
    'https://jsr.corp.example/': { scopes: ['@jsr'] },
    'https://npm.corp.example/': {
      serverType: 'artifactory',
      scopes: ['@', '@acme'],
      prefix: 'work',
    },
    'https://npm.pkg.github.com/': { prefix: 'gh' },
    'https://registry.npmjs.org/': { prefix: 'npmjs' },
  })
})

test('toResolvedRegistryDeclarations() lets a user prefix win over the built-in of the same name', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByPrefix: { gh: 'https://github.corp.example/' },
  })).toStrictEqual({
    'https://github.corp.example/': { prefix: 'gh' },
    'https://npm.jsr.io/': { scopes: ['@jsr'] },
    'https://registry.npmjs.org/': { scopes: ['@'], prefix: 'npmjs' },
  })
})

test('toResolvedRegistryDeclarations() emits keys, fields, and scopes in canonical order', () => {
  // Both CLI implementations print this view as JSON, so its ordering is part
  // of the shared contract: registry URLs and scope lists lexicographic,
  // fields as the setting documents them, and the alphabetically last of the
  // prefixes routed to one URL.
  const resolved = toResolvedRegistryDeclarations({
    registriesByScope: {
      '@other': 'https://npm.other.example/',
      '@acme': 'https://npm.corp.example/',
      default: 'https://npm.corp.example/',
    },
    registriesByPrefix: { work: 'https://npm.corp.example/', corp: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { supportsTimeField: true, serverType: 'artifactory' } },
  })
  expect(Object.keys(resolved)).toStrictEqual([
    'https://npm.corp.example/',
    'https://npm.jsr.io/',
    'https://npm.other.example/',
    'https://npm.pkg.github.com/',
    'https://registry.npmjs.org/',
  ])
  expect(Object.keys(resolved['https://npm.corp.example/'])).toStrictEqual(['serverType', 'supportsTimeField', 'scopes', 'prefix'])
  expect(resolved['https://npm.corp.example/'].scopes).toStrictEqual(['@', '@acme'])
  expect(resolved['https://npm.corp.example/'].prefix).toBe('work')
})
