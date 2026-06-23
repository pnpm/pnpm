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

test('createNpmResolutionVerifier() still verifies tarball URLs when no age/trust policy is active', async () => {
  // The tarball-URL binding is unconditional, so the verifier exists even
  // with no minimumReleaseAge/trustPolicy configured.
  const meta = {
    name: 'aged-pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'aged-pkg',
        version: '1.0.0',
        dist: { tarball: 'https://registry.npmjs.org/aged-pkg/-/aged-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/aged-pkg', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  expect(verifier).toBeDefined()
  const result = await verifier.verify(
    {
      integrity: FAKE_INTEGRITY,
      tarball: 'https://attacker.example/aged-pkg-1.0.0.tgz',
    } as unknown as Resolution,
    { name: 'aged-pkg', version: '1.0.0' }
  )
  expect(result).toMatchObject({ ok: false, code: 'TARBALL_URL_MISMATCH' })
})

test('createNpmResolutionVerifier() passes package name to auth header lookup', async () => {
  const tarball = 'https://registry.npmjs.org/@scope/pkg/-/pkg-1.0.0.tgz'
  const meta = {
    name: '@scope/pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: '@scope/pkg',
        version: '1.0.0',
        dist: { tarball, shasum: 'aa' },
      },
    },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({
    path: `/@scope${'%2F'}pkg`,
    method: 'GET',
    headers: { authorization: 'Bearer scoped-token' },
  }).reply(200, meta).persist()

  const calls: Array<{ uri: string, pkgName?: string }> = []
  const scopedGetAuthHeader = (uri: string, opts?: { pkgName?: string }): string | undefined => {
    calls.push({ uri, pkgName: opts?.pkgName })
    return opts?.pkgName === '@scope/pkg' ? 'Bearer scoped-token' : undefined
  }
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({ getAuthHeaderValueByURI: scopedGetAuthHeader }))
  const result = await verifier.verify(
    { integrity: FAKE_INTEGRITY, tarball } as unknown as Resolution,
    { name: '@scope/pkg', version: '1.0.0' }
  )
  expect(result).toStrictEqual({ ok: true })
  expect(calls).toContainEqual({ uri: registries.default, pkgName: '@scope/pkg' })
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
  }))
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
  }))
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
  }))
  // Registry-style resolution (no explicit tarball URL) so the entry
  // exercises the age check's abbreviated shortcut rather than the
  // tarball-URL binding (which would fail closed on the missing version
  // first).
  const result = await verifier.verify(
    { integrity: FAKE_INTEGRITY } as unknown as Resolution,
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
  }))
  const result = await verifier.verify(
    makeTarballResolution('time-free-pkg', '1.0.0'),
    { name: 'time-free-pkg', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() skips file: tarball resolutions', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))
  const result = await verifier.verify(
    {
      integrity: 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==',
      tarball: 'file:vendor/types__my-cool-lib-v1.0.0.tgz',
    } as unknown as Resolution,
    { name: '@types/my-cool-lib', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

const REGISTRY_TARBALL = 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz'

test('createNpmResolutionVerifier() rejects a registry tarball with no integrity', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { tarball: REGISTRY_TARBALL } as unknown as Resolution,
    { name: 'foo', version: '1.0.0' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() rejects a canonical registry entry stripped down to {}', async () => {
  // A tampered lockfile can delete both the tarball URL and integrity from a canonical
  // registry entry; the URL is reconstructed from name+version, so it must still be rejected.
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify({} as unknown as Resolution, { name: 'foo', version: '1.0.0' })
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() treats an empty-string integrity as missing', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { integrity: '', tarball: REGISTRY_TARBALL } as unknown as Resolution,
    { name: 'foo', version: '1.0.0' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() treats a non-string integrity as missing', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  for (const integrity of [true, [], {}] as unknown[]) {
    // eslint-disable-next-line no-await-in-loop
    const result = await verifier.verify(
      { integrity, tarball: REGISTRY_TARBALL } as unknown as Resolution,
      { name: 'foo', version: '1.0.0' }
    )
    expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
  }
})

test('createNpmResolutionVerifier() enforces missing integrity even with a non-semver version', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { tarball: REGISTRY_TARBALL } as unknown as Resolution,
    { name: 'foo', version: 'not-a-semver' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() exempts a git-hosted tarball URL recorded without the gitHosted flag', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/0123456789abcdef0123456789abcdef01234567' } as unknown as Resolution,
    { name: 'is-negative', version: '1.0.0', nonSemverVersion: 'https+++github.com+kevva+is-negative' }
  )
  expect(result).toStrictEqual({ ok: true })
})

test('createNpmResolutionVerifier() rejects git-host archive URLs that are not pinned to a commit', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/main' } as unknown as Resolution,
    { name: 'is-negative', version: '1.0.0', nonSemverVersion: 'https+++github.com+kevva+is-negative' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() rejects a forged gitHosted flag on a non-git-hosted tarball', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { gitHosted: true, tarball: 'https://attacker.example/evil-1.0.0.tgz' } as unknown as Resolution,
    { name: 'evil', version: '1.0.0', nonSemverVersion: 'https+++attacker.example+evil' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() enforces missing integrity on a URL-keyed (nonSemverVersion) tarball', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { tarball: 'https://cdn.example/foo/-/foo-1.0.0.tgz' } as unknown as Resolution,
    { name: 'foo', version: '1.0.0', nonSemverVersion: 'https://cdn.example/foo/-/foo-1.0.0.tgz' }
  )
  expect(result).toMatchObject({ ok: false, code: 'MISSING_TARBALL_INTEGRITY' })
})

test('createNpmResolutionVerifier() passes a URL-keyed tarball that carries integrity without a registry lookup', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { integrity: FAKE_INTEGRITY, tarball: 'https://cdn.example/foo/-/foo-1.0.0.tgz' } as unknown as Resolution,
    { name: 'foo', version: '1.0.0', nonSemverVersion: 'https://cdn.example/foo/-/foo-1.0.0.tgz' }
  )
  expect(result).toStrictEqual({ ok: true })
})

test('createNpmResolutionVerifier() fails closed on a non-semver version for a registry tarball', async () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { integrity: FAKE_INTEGRITY, tarball: REGISTRY_TARBALL } as unknown as Resolution,
    { name: 'foo', version: 'not-a-semver' }
  )
  expect(result).toMatchObject({ ok: false, code: 'TARBALL_URL_MISMATCH' })
})

test('createNpmResolutionVerifier() rejects a non-string tarball instead of crashing', async () => {
  // A YAML array `tarball` would otherwise be string-coerced into an attacker URL later;
  // the verifier fails closed rather than silently skipping the URL-binding check.
  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    { integrity: FAKE_INTEGRITY, tarball: ['https://attacker.example/foo-1.0.0.tgz'] } as unknown as Resolution,
    { name: 'foo', version: '1.0.0' }
  )
  expect(result).toMatchObject({ ok: false, code: 'TARBALL_URL_MISMATCH' })
})

const FAKE_INTEGRITY = 'sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=='

test('createNpmResolutionVerifier() flags a lockfile tarball URL that does not match the registry metadata', async () => {
  // The version is old enough to clear minimumReleaseAge, but the lockfile
  // pins a tarball URL on a host the registry never published to. A tampered
  // lockfile could pair an aged, trusted name@version with attacker-hosted
  // bytes; the verifier must reject the entry before the age check passes it.
  const meta = {
    name: 'aged-pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'aged-pkg',
        version: '1.0.0',
        dist: { tarball: 'https://registry.npmjs.org/aged-pkg/-/aged-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/aged-pkg', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))
  const result = await verifier.verify(
    {
      integrity: FAKE_INTEGRITY,
      tarball: 'https://attacker.example/aged-pkg-1.0.0.tgz',
    } as unknown as Resolution,
    { name: 'aged-pkg', version: '1.0.0' }
  )
  expect(result).toMatchObject({
    ok: false,
    code: 'TARBALL_URL_MISMATCH',
  })
})

test('createNpmResolutionVerifier() accepts a non-standard tarball URL that matches the registry metadata', async () => {
  // npm Enterprise / GitHub Packages serve tarballs from a path the default
  // URL template can't reconstruct, so the lockfile keeps the URL. As long
  // as it's the URL the registry's own metadata lists, it's legitimate.
  const meta = {
    name: 'enterprise-pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'enterprise-pkg',
        version: '1.0.0',
        dist: { tarball: 'https://registry.npmjs.org/enterprise-pkg/download/enterprise-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/enterprise-pkg', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))
  const result = await verifier.verify(
    {
      integrity: FAKE_INTEGRITY,
      tarball: 'https://registry.npmjs.org/enterprise-pkg/download/enterprise-pkg-1.0.0.tgz',
    } as unknown as Resolution,
    { name: 'enterprise-pkg', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() treats a default-port / scheme difference as a match', async () => {
  // The lockfile URL and the registry metadata differ only by an explicit
  // default port and the http/https scheme — benign normalizations, not
  // tampering — so `sameTarballUrl` must canonicalize them away.
  const meta = {
    name: 'aged-pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'aged-pkg',
        version: '1.0.0',
        dist: { tarball: 'http://registry.npmjs.org:80/aged-pkg/-/aged-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    time: { '1.0.0': '2020-01-01T00:00:00.000Z' },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/aged-pkg', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))
  const result = await verifier.verify(
    {
      integrity: FAKE_INTEGRITY,
      tarball: 'https://registry.npmjs.org/aged-pkg/-/aged-pkg-1.0.0.tgz',
    } as unknown as Resolution,
    { name: 'aged-pkg', version: '1.0.0' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() skips URL-keyed tarball deps even when they carry a semver version', async () => {
  // A remote `https:` tarball dependency keeps a semver `version` copied from
  // its manifest, but its lockfile key is the URL (nonSemverVersion). It is a
  // deliberate non-registry dep: neither the release-age policy nor the
  // registry tarball-URL binding applies, and no registry lookup should fire.
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    minimumReleaseAge: 1440,
  }))
  const result = await verifier.verify(
    {
      integrity: FAKE_INTEGRITY,
      tarball: 'https://example.com/foo-1.0.0.tgz',
    } as unknown as Resolution,
    { name: 'foo', version: '1.0.0', nonSemverVersion: 'https://example.com/foo-1.0.0.tgz' }
  )
  expect(result).toEqual({ ok: true })
})

test('createNpmResolutionVerifier() canTrustPastCheck rejects when the trust-exclude list shrinks', () => {
  const verifier = createNpmResolutionVerifier(makeVerifierOpts({
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo'],
  }))
  // Same policy → trust.
  expect(verifier.canTrustPastCheck({
    tarballUrlBinding: true,
    integrityRequired: true,
    minimumReleaseAge: 0,
    minimumReleaseAgeExclude: [],
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo'],
    trustPolicyIgnoreAfter: null,
  })).toBe(true)
  // Cached run had a wider exclude list (today's is stricter) → invalidate.
  expect(verifier.canTrustPastCheck({
    tarballUrlBinding: true,
    integrityRequired: true,
    minimumReleaseAge: 0,
    minimumReleaseAgeExclude: [],
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['foo', 'bar'],
    trustPolicyIgnoreAfter: null,
  })).toBe(false)
})

test('createNpmResolutionVerifier() propagates the registry fetch error instead of reporting a tampering-style mismatch', async () => {
  // A 403 on the metadata fetch (e.g. a CI token that is authenticated but not
  // authorized to read a private package) must not be reported as a lockfile
  // tarball-URL mismatch: the lockfile is correct, the fetch is the problem.
  // The verifier rethrows the registry's own error so the install aborts with
  // ERR_PNPM_FETCH_403 (which already explains the auth situation).
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/private-pkg', method: 'GET' }).reply(403, { error: 'Forbidden' }).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  await expect(verifier.verify(
    makeTarballResolution('private-pkg', '1.0.0'),
    { name: 'private-pkg', version: '1.0.0' }
  )).rejects.toMatchObject({ code: 'ERR_PNPM_FETCH_403' })
})

test('createNpmResolutionVerifier() still flags a version absent from fetched metadata as TARBALL_URL_MISMATCH', async () => {
  // The metadata fetch succeeds but does not list the pinned version. That is a
  // genuine verification failure (not a transport error), so it must stay
  // TARBALL_URL_MISMATCH — distinct from the new TARBALL_URL_FETCH_FAILED.
  const meta = {
    name: 'present-pkg',
    'dist-tags': { latest: '1.0.0' },
    versions: {
      '1.0.0': {
        name: 'present-pkg',
        version: '1.0.0',
        dist: { tarball: 'https://registry.npmjs.org/present-pkg/-/present-pkg-1.0.0.tgz', shasum: 'aa' },
      },
    },
    modified: '2020-01-01T00:00:00.000Z',
  }
  const pool = getMockAgent().get('https://registry.npmjs.org')
  pool.intercept({ path: '/present-pkg', method: 'GET' }).reply(200, meta).persist()

  const verifier = createNpmResolutionVerifier(makeVerifierOpts())
  const result = await verifier.verify(
    makeTarballResolution('present-pkg', '2.0.0'),
    { name: 'present-pkg', version: '2.0.0' }
  )

  expect(result).toMatchObject({ ok: false, code: 'TARBALL_URL_MISMATCH' })
})
