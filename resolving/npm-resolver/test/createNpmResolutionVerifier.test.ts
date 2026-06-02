import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import { createNpmResolutionVerifier } from '@pnpm/resolving.npm-resolver'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import type { Registries } from '@pnpm/types'
import { temporaryDirectory } from 'tempy'

import { getMockAgent, setupMockAgent, teardownMockAgent } from './utils/index.js'

const registries: Registries = {
  default: 'https://registry.npmjs.org/',
}

const fetchFromRegistry = createFetchFromRegistry({})
const getAuthHeaderValueByURI = (): undefined => undefined

function makeVerifierOpts (overrides: Partial<Parameters<typeof createNpmResolutionVerifier>[0]> = {}): Parameters<typeof createNpmResolutionVerifier>[0] {
  return {
    registries,
    fetchOpts: {
      fetch: fetchFromRegistry,
      retry: { retries: 0 },
      timeout: 60_000,
      fetchWarnTimeoutMs: 10_000,
    },
    getAuthHeaderValueByURI,
    cacheDir: temporaryDirectory(),
    now: Date.UTC(2026, 0, 1),
    ...overrides,
  }
}

function makeTarballResolution (name: string, version: string): Resolution {
  return {
    integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
    tarball: `https://registry.npmjs.org/${name}/-/${name}-${version}.tgz`,
  } as unknown as Resolution
}

afterEach(async () => {
  await teardownMockAgent()
})

beforeEach(async () => {
  await setupMockAgent()
})

test('createNpmResolutionVerifier() returns undefined when no policy is active', () => {
  expect(createNpmResolutionVerifier(makeVerifierOpts())).toBeUndefined()
})

test('createNpmResolutionVerifier() flags a trustedPublisher → provenance downgrade', async () => {
  // 0.0.1 was published by a trustedPublisher with provenance → rank 2.
  // 0.0.2 is provenance-only (rank 1, weaker) → downgrade vs 0.0.1.
  // This is exactly the case the resolver-time trustChecks unit tests
  // cover, but routed through the lockfile verifier. The verifier must
  // not pass simply because 0.0.2 has *some* attestation.
  const meta = {
    name: 'demo',
    'dist-tags': { latest: '0.0.2' },
    versions: {
      '0.0.1': {
        name: 'demo',
        version: '0.0.1',
        dist: {
          tarball: 'https://registry.npmjs.org/demo/-/demo-0.0.1.tgz',
          shasum: 'aa',
          attestations: { provenance: { predicateType: 'https://example.org/p' } },
        },
        _npmUser: { trustedPublisher: { id: 'gha', oidcConfigId: 'cfg' } },
      },
      '0.0.2': {
        name: 'demo',
        version: '0.0.2',
        dist: {
          tarball: 'https://registry.npmjs.org/demo/-/demo-0.0.2.tgz',
          shasum: 'bb',
          attestations: { provenance: { predicateType: 'https://example.org/p' } },
        },
      },
    },
    time: {
      '0.0.1': '2025-01-01T00:00:00.000Z',
      '0.0.2': '2025-06-01T00:00:00.000Z',
    },
    modified: '2025-06-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/demo', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    trustPolicy: 'no-downgrade',
  }))!
  expect(verifier).toBeDefined()

  const result = await verifier.verify(makeTarballResolution('demo', '0.0.2'), { name: 'demo', version: '0.0.2' })
  expect(result).toMatchObject({
    ok: false,
    code: 'TRUST_DOWNGRADE',
  })
})

test('createNpmResolutionVerifier() passes a same-evidence-level version', async () => {
  // 0.0.1 had provenance, 0.0.2 still has provenance → no downgrade.
  // Verifies the trust check isn't over-aggressive for stable evidence.
  const meta = {
    name: 'demo',
    'dist-tags': { latest: '0.0.2' },
    versions: {
      '0.0.1': {
        name: 'demo',
        version: '0.0.1',
        dist: {
          tarball: 'https://registry.npmjs.org/demo/-/demo-0.0.1.tgz',
          shasum: 'aa',
          attestations: { provenance: { predicateType: 'https://example.org/p1' } },
        },
      },
      '0.0.2': {
        name: 'demo',
        version: '0.0.2',
        dist: {
          tarball: 'https://registry.npmjs.org/demo/-/demo-0.0.2.tgz',
          shasum: 'bb',
          attestations: { provenance: { predicateType: 'https://example.org/p2' } },
        },
      },
    },
    time: {
      '0.0.1': '2025-01-01T00:00:00.000Z',
      '0.0.2': '2025-06-01T00:00:00.000Z',
    },
    modified: '2025-06-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/demo', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    trustPolicy: 'no-downgrade',
  }))!
  const result = await verifier.verify(makeTarballResolution('demo', '0.0.2'), { name: 'demo', version: '0.0.2' })
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() abbreviated shortcut requires the pinned version to be in metadata', async () => {
  // Package's `modified` is well before the cutoff (default 1-day window
  // means modified=2010 is fine), but `0.0.2` was unpublished and is no
  // longer in `versions`. The shortcut must NOT return the package-level
  // `modified` for that version — that would be a fail-open on a
  // missing pin. The verifier should fall through to the deeper layers
  // and end up reporting a violation (no source could surface the time).
  const abbreviatedMeta = {
    name: 'unpublished-pkg',
    'dist-tags': {},
    versions: {
      '0.0.1': {
        name: 'unpublished-pkg',
        version: '0.0.1',
        dist: { tarball: 'https://registry.npmjs.org/unpublished-pkg/-/unpublished-pkg-0.0.1.tgz', shasum: 'aa' },
      },
    },
    modified: '2010-01-01T00:00:00.000Z',
  }
  const fullMeta = {
    ...abbreviatedMeta,
    time: { '0.0.1': '2010-01-01T00:00:00.000Z' },
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/unpublished-pkg', method: 'GET' }).reply(200, abbreviatedMeta).persist()
  pool.intercept({ path: '/-/npm/v1/attestations/unpublished-pkg@0.0.2', method: 'GET' }).reply(404, {}).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440, // 1 day
  }))!
  const result = await verifier.verify(
    makeTarballResolution('unpublished-pkg', '0.0.2'),
    { name: 'unpublished-pkg', version: '0.0.2' }
  )
  expect(result).toMatchObject({
    ok: false,
    code: 'MINIMUM_RELEASE_AGE_VIOLATION',
  })

  // Sanity check: the unrelated full meta isn't used here because the
  // abbreviated shortcut won't fire (version missing), and the deeper
  // layers also have no entry for 0.0.2. Keep `fullMeta` in scope so
  // future test additions can wire it in without redefining.
  expect(fullMeta.versions['0.0.1'].version).toBe('0.0.1')
})

test('createNpmResolutionVerifier() ignoreMissingTimeField passes the entry when no source surfaces a timestamp', async () => {
  // Mirrors the resolver-side `pickMatchingVersionFinal` warn-and-skip
  // behavior: when the registry strips the per-version `time` field and
  // the user has opted into `minimumReleaseAgeIgnoreMissingTime`, the
  // verifier shouldn't be stricter than fresh resolution.
  const abbreviatedMeta = {
    name: 'time-free-pkg',
    'dist-tags': {},
    versions: {
      '1.0.0': {
        name: 'time-free-pkg',
        version: '1.0.0',
        dist: { tarball: 'https://registry.npmjs.org/time-free-pkg/-/time-free-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    modified: '2010-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  // Full meta also lacks `time`, so no layer surfaces a publish timestamp.
  pool.intercept({ path: '/time-free-pkg', method: 'GET' }).reply(200, abbreviatedMeta).persist()
  pool.intercept({ path: '/-/npm/v1/attestations/time-free-pkg@1.0.0', method: 'GET' }).reply(404, {}).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
    ignoreMissingTimeField: true,
  }))!
  const result = await verifier.verify(
    makeTarballResolution('time-free-pkg', '1.0.0'),
    { name: 'time-free-pkg', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() skips file: tarball resolutions', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))!
  const result = await verifier.verify(
    {
      integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
      tarball: 'file:vendor/types__my-cool-lib-v1.0.0.tgz',
    } as unknown as Resolution,
    { name: '@types/my-cool-lib', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() canTrustPastCheck rejects when the trust-exclude list shrinks', () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo'],
  }))!
  // Same policy → trust.
  expect(verifier.canTrustPastCheck({
    minimumReleaseAge: 0,
    minimumReleaseAgeExclude: [],
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo'],
    trustPolicyIgnoreAfter: null,
  })).toBe(true)
  // Cached run had a wider exclude list (today's is stricter) → invalidate.
  expect(verifier.canTrustPastCheck({
    minimumReleaseAge: 0,
    minimumReleaseAgeExclude: [],
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo', 'bar'],
    trustPolicyIgnoreAfter: null,
  })).toBe(false)
})
