import { expect, test } from '@jest/globals'

import { needsFullMetadataForRegistry, shouldFetchFullMetadata, shouldFilterMetadata } from '../src/createNewStoreController.js'

test('returns false by default', () => {
  expect(shouldFetchFullMetadata({})).toBe(false)
})

test('an explicit fetchFullMetadata overrides every derived reason', () => {
  expect(shouldFetchFullMetadata({ fetchFullMetadata: true })).toBe(true)
  expect(shouldFetchFullMetadata({
    fetchFullMetadata: false,
    trustPolicy: 'no-downgrade',
    resolutionMode: 'time-based',
    supportedArchitectures: { libc: ['glibc'] },
  })).toBe(false)
})

test('returns true when supportedArchitectures.libc is set', () => {
  expect(shouldFetchFullMetadata({ supportedArchitectures: { libc: ['glibc'] } })).toBe(true)
  // The libc field is missing from abbreviated metadata regardless of
  // whether the registry includes the time field in it.
  expect(shouldFetchFullMetadata({
    supportedArchitectures: { libc: ['glibc'] },
    registrySupportsTimeField: true,
  })).toBe(true)
})

test('returns false when supportedArchitectures is set without libc', () => {
  expect(shouldFetchFullMetadata({ supportedArchitectures: { os: ['darwin'] } })).toBe(false)
})

test('returns true when trustPolicy is no-downgrade', () => {
  expect(shouldFetchFullMetadata({ trustPolicy: 'no-downgrade' })).toBe(true)
})

// Regression test for https://github.com/pnpm/pnpm/issues/12883:
// global installs computed this flag from supportedArchitectures alone and
// passed `false`, which suppressed the trustPolicy fallback and made every
// resolution fail with ERR_PNPM_MISSING_TIME.
test('trustPolicy requires full metadata even when supportedArchitectures is set without libc', () => {
  expect(shouldFetchFullMetadata({
    trustPolicy: 'no-downgrade',
    supportedArchitectures: {},
  })).toBe(true)
})

test('returns true when resolutionMode is time-based', () => {
  expect(shouldFetchFullMetadata({ resolutionMode: 'time-based' })).toBe(true)
})

test('a registry whose abbreviated metadata has the time field needs no full metadata for time-based resolution', () => {
  expect(shouldFetchFullMetadata({
    resolutionMode: 'time-based',
    registrySupportsTimeField: true,
  })).toBe(false)
})

// Trust checks read trust evidence (_npmUser) that abbreviated metadata
// never carries, so registrySupportsTimeField does not make abbreviated
// metadata sufficient for the no-downgrade policy. This matches the
// self-update code path and pacquet's
// Config::requires_full_metadata_for_resolution.
test('trustPolicy requires full metadata even when the registry has the time field in abbreviated metadata', () => {
  expect(shouldFetchFullMetadata({
    trustPolicy: 'no-downgrade',
    registrySupportsTimeField: true,
  })).toBe(true)
})

const PUBLIC_REGISTRY = 'https://registry.npmjs.org/'
const TIME_REGISTRY = 'https://time.example.com/'

test('a registry that declares the time field answers for itself, without exempting the others', () => {
  const needsFullMetadata = needsFullMetadataForRegistry({
    resolutionMode: 'time-based',
    registryOptionsByUrl: { [TIME_REGISTRY]: { supportsTimeField: true } },
  })

  expect(needsFullMetadata(TIME_REGISTRY)).toBe(false)
  expect(needsFullMetadata(PUBLIC_REGISTRY)).toBe(true)
})

test('a registry with no declaration answers what the setting answers', () => {
  const needsFullMetadata = needsFullMetadataForRegistry({
    resolutionMode: 'time-based',
    registrySupportsTimeField: true,
    registryOptionsByUrl: { [TIME_REGISTRY]: { supportsTimeField: false } },
  })

  expect(needsFullMetadata(PUBLIC_REGISTRY)).toBe(false)
  // A declaration overrides the setting in both directions.
  expect(needsFullMetadata(TIME_REGISTRY)).toBe(true)
})

test('a reason that holds for every registry is not undone by a declaration', () => {
  const needsFullMetadata = needsFullMetadataForRegistry({
    resolutionMode: 'time-based',
    trustPolicy: 'no-downgrade',
    registryOptionsByUrl: { [TIME_REGISTRY]: { supportsTimeField: true } },
  })

  expect(needsFullMetadata(TIME_REGISTRY)).toBe(true)
})

test('the declaration is matched however either side spelled the trailing slash', () => {
  const needsFullMetadata = needsFullMetadataForRegistry({
    resolutionMode: 'time-based',
    registryOptionsByUrl: { [TIME_REGISTRY]: { supportsTimeField: true } },
  })

  expect(needsFullMetadata('https://time.example.com')).toBe(false)
})

// The filtered mirror is chosen once for the whole client, so it has to cover
// the registry that asks for the most: one the setting exempts but a
// declaration does not.
test('the filtered mirror is used for a registry that needs full metadata though the setting exempts it', () => {
  const opts = {
    resolutionMode: 'time-based',
    registrySupportsTimeField: true,
    registryOptionsByUrl: { [TIME_REGISTRY]: { supportsTimeField: false } },
  } as const

  expect(shouldFetchFullMetadata(opts)).toBe(false)
  expect(needsFullMetadataForRegistry(opts)(TIME_REGISTRY)).toBe(true)
  expect(shouldFilterMetadata(opts)).toBe(true)
})

test('nothing is filtered when no registry can need full metadata', () => {
  expect(shouldFilterMetadata({ resolutionMode: 'highest' })).toBe(false)
  expect(shouldFilterMetadata({ resolutionMode: 'time-based', fetchFullMetadata: false })).toBe(false)
})
