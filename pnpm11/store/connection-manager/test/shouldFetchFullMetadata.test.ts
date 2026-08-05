import { expect, test } from '@jest/globals'

import { shouldFetchFullMetadata } from '../src/createNewStoreController.js'

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
