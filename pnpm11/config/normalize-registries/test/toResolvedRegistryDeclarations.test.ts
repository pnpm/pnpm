import { expect, test } from '@jest/globals'
import { toResolvedRegistryDeclarations } from '@pnpm/config.normalize-registries'

test('toResolvedRegistryDeclarations() declares the default registry as the bare @ scope', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByScope: { default: 'https://registry.npmjs.org/' },
  })).toStrictEqual({
    'https://registry.npmjs.org/': { scopes: ['@'] },
  })
})

test('toResolvedRegistryDeclarations() merges the default route into the registry entry it belongs to', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByScope: {
      default: 'https://npm.corp.example/',
      '@acme': 'https://npm.corp.example/',
    },
    registriesByPrefix: { work: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { serverType: 'artifactory' } },
  })).toStrictEqual({
    'https://npm.corp.example/': {
      serverType: 'artifactory',
      scopes: ['@', '@acme'],
      prefix: 'work',
    },
  })
})

test('toResolvedRegistryDeclarations() still omits a built-in route the user did not point elsewhere', () => {
  expect(toResolvedRegistryDeclarations({
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@jsr': 'https://npm.jsr.io/',
    },
  })).toStrictEqual({
    'https://registry.npmjs.org/': { scopes: ['@'] },
  })
})

test('toResolvedRegistryDeclarations() emits keys, fields, and scopes in canonical order', () => {
  // Both CLI implementations print this view as JSON, so its ordering is part
  // of the shared contract: registry URLs and scope lists lexicographic,
  // fields as the setting documents them.
  const resolved = toResolvedRegistryDeclarations({
    registriesByScope: {
      '@other': 'https://npm.other.example/',
      '@acme': 'https://npm.corp.example/',
      default: 'https://npm.corp.example/',
    },
    registriesByPrefix: { work: 'https://npm.corp.example/' },
    registryOptionsByUrl: { 'https://npm.corp.example/': { supportsTimeField: true, serverType: 'artifactory' } },
  })
  expect(Object.keys(resolved)).toStrictEqual(['https://npm.corp.example/', 'https://npm.other.example/'])
  expect(Object.keys(resolved['https://npm.corp.example/'])).toStrictEqual(['serverType', 'supportsTimeField', 'scopes', 'prefix'])
  expect(resolved['https://npm.corp.example/'].scopes).toStrictEqual(['@', '@acme'])
})
