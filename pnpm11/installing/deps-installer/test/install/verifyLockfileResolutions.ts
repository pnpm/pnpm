import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION } from '@pnpm/constants'
import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import { createPeerDepGraphHash } from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { DepPath } from '@pnpm/types'

import { matchReplacedLockfileEntries } from '../../src/install/installPredicates.js'
import { settlePreparedMutationHooksAndVerification } from '../../src/install/preparedFrozen.js'
import { recordLockfileVerified } from '../../src/install/recordLockfileVerified.js'
import {
  buildExoticSubdepsScopeFromContext,
  buildExoticSubdepsScopeFromProjectDirs,
  computeExoticSubdepsScopeDigest,
  joinPosixRelPath,
  ownStringProp,
} from '../../src/install/verifyLockfileExoticSubdepsScope.js'
import {
  canTrustPreparedPolicy,
  snapshotVerificationPolicy,
  verifyLockfileResolutions,
} from '../../src/install/verifyLockfileResolutions.js'
import { testDefaults } from '../utils/index.js'

const GIT_COMMIT = '0123456789abcdef0123456789abcdef01234567'

function makeLockfile (
  packages: Record<string, { resolution: unknown, version?: string }>,
  lockfileVersion: string = LOCKFILE_VERSION
): LockfileObject {
  return {
    lockfileVersion,
    importers: {},
    packages: packages as LockfileObject['packages'],
  } as LockfileObject
}

const tarballResolution = (integrity: string = 'sha512-deadbeef') => ({ integrity, tarball: '' })

const NOOP_SLOT = {
  policy: {} as Record<string, unknown>,
  canTrustPastCheck: () => true,
}

function wrap (
  verify: ResolutionVerifier['verify'],
  slot: Omit<ResolutionVerifier, 'verify'> = NOOP_SLOT
): ResolutionVerifier {
  return { ...slot, verify }
}

const okVerifier = wrap(async () => ({ ok: true }))

test('no-op when the verifier list is empty', async () => {
  const lockfile = makeLockfile({
    'fresh@1.0.0': { resolution: tarballResolution() },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('no-op when lockfile has no packages', async () => {
  const lockfile = makeLockfile({})
  await expect(verifyLockfileResolutions(lockfile, [okVerifier])).resolves.toBeUndefined()
})

test('passes when every entry is verified ok', async () => {
  const lockfile = makeLockfile({
    'lodash@4.17.21': { resolution: tarballResolution() },
    'is-odd@0.1.0': { resolution: tarballResolution() },
  })
  await expect(verifyLockfileResolutions(lockfile, [okVerifier])).resolves.toBeUndefined()
})

test('throws with the verifier-supplied code and reason on a single failure', async () => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution() },
  })
  const verifier = wrap(async () => ({
    ok: false,
    code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    reason: 'was published yesterday',
  }))

  await expect(verifyLockfileResolutions(lockfile, [verifier])).rejects.toMatchObject({
    code: 'ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION',
    hint: expect.stringMatching(/If the changes look expected[\s\S]*If the fresh resolution still fails and you trust the affected packages, relax the policy that flagged them\./),
    message: expect.stringMatching(/is-odd@0\.1\.2 was published yesterday/),
  })
})

test('propagates a verifier throw (registry fetch failure) instead of folding it into a batch', async () => {
  // A verifier throws — rather than returning a violation — when it can't reach
  // the registry to verify an entry. That transport error must surface as-is
  // (the install aborts with the registry's own error), not be turned into a
  // lockfile-verification batch.
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution('sha512-a') },
    'private-pkg@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const fetchError = Object.assign(new Error('GET https://registry.example/private-pkg: Forbidden - 403'), {
    code: 'ERR_PNPM_FETCH_403',
  })
  const verifier = wrap(async (_, { name }) => {
    if (name === 'private-pkg') throw fetchError
    return { ok: false, code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'too fresh' }
  })

  // The thrown transport error wins over the collected policy violation.
  await expect(verifyLockfileResolutions(lockfile, [verifier])).rejects.toBe(fetchError)
})

test('throws a generic code with per-entry codes in the breakdown when violations span policies', async () => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution('sha512-a') },
    'untrusted@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const verifier = wrap(async (_, { name }) => {
    if (name === 'is-odd') {
      return { ok: false, code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'too fresh' }
    }
    return { ok: false, code: 'TRUST_DOWNGRADE', reason: 'trust weakened' }
  })

  await expect(verifyLockfileResolutions(lockfile, [verifier])).rejects.toMatchObject({
    // Mixed-code batch escalates to the generic LOCKFILE_RESOLUTION_VERIFICATION
    // code so downstream handlers don't branch on whichever entry happened
    // to land first.
    code: 'ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION',
    // Per-entry code is included in the breakdown so the user can see
    // which policy each line tripped.
    message: expect.stringMatching(/is-odd@0\.1\.2 \[MINIMUM_RELEASE_AGE_VIOLATION\][\s\S]*untrusted@1\.0\.0 \[TRUST_DOWNGRADE\]/),
  })
})

test.each([
  'MISSING_TARBALL_INTEGRITY',
  'TARBALL_URL_MISMATCH',
  'MISSING_NAMED_REGISTRY',
])('does not suggest relaxing a policy for %s, alone or in a mixed batch', async (code) => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution('sha512-a') },
    'broken@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const structuralOnly = wrap(async (_, { name }) =>
    name === 'broken' ? { ok: false, code, reason: 'broken' } : { ok: true }
  )
  const mixed = wrap(async (_, { name }) =>
    name === 'broken'
      ? { ok: false, code, reason: 'broken' }
      : { ok: false, code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'too fresh' }
  )

  await expect(verifyLockfileResolutions(lockfile, [structuralOnly])).rejects.toMatchObject({
    code: `ERR_PNPM_${code}`,
    hint: expect.not.stringMatching(/relax the policy/),
  })
  await expect(verifyLockfileResolutions(lockfile, [mixed])).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION',
    hint: expect.not.stringMatching(/relax the policy/),
  })
})

test('does not suggest relaxing a policy when a mixed batch includes a structural violation', async () => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution('sha512-a') },
    'no-integrity@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const verifier = wrap(async (_, { name }) => {
    if (name === 'is-odd') {
      return { ok: false, code: 'MINIMUM_RELEASE_AGE_VIOLATION', reason: 'too fresh' }
    }
    return { ok: false, code: 'MISSING_TARBALL_INTEGRITY', reason: 'has no "integrity" field' }
  })

  await expect(verifyLockfileResolutions(lockfile, [verifier])).rejects.toMatchObject({
    code: 'ERR_PNPM_LOCKFILE_RESOLUTION_VERIFICATION',
    hint: expect.not.stringMatching(/relax the policy/),
  })
})

test('lists violations in stable order across multiple failures', async () => {
  const lockfile = makeLockfile({
    'fresh-b@2.0.0': { resolution: tarballResolution('sha512-b') },
    'fresh-a@1.0.0': { resolution: tarballResolution('sha512-a') },
  })
  const verifier = wrap(async (_, { name, version }) => ({
    ok: false,
    code: 'POLICY_X',
    reason: `${name}@${version} failed`,
  }))

  await expect(verifyLockfileResolutions(lockfile, [verifier]))
    .rejects.toThrow(/fresh-a@1\.0\.0[\s\S]*fresh-b@2\.0\.0/)
})

test('caps printed violations at 20 with an "…and N more" summary', async () => {
  const packages: Record<string, { resolution: unknown }> = {}
  for (let index = 0; index < 25; index++) {
    packages[`pkg-${String(index).padStart(2, '0')}@1.0.0`] = {
      resolution: tarballResolution(`sha512-${index}`),
    }
  }
  const lockfile = makeLockfile(packages)
  const verifier = wrap(async (_, { name, version }) => ({
    ok: false,
    code: 'POLICY_X',
    reason: `${name}@${version}`,
  }))

  await expect(verifyLockfileResolutions(lockfile, [verifier]))
    .rejects.toThrow(/25 lockfile entries failed verification[\s\S]*…and 5 more/)
})

test('dedupes peer/patch-suffix variants and invokes the verifier once per (name, version)', async () => {
  const lockfile = makeLockfile({
    'react@18.0.0': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(peer-x)': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(patch_hash=abc)(peer-x)': { resolution: tarballResolution('sha512-a') },
  })
  const seen: Array<{ name: string, version: string }> = []
  const verifier = wrap(async (_, { name, version }) => {
    seen.push({ name, version })
    return { ok: true }
  })

  await verifyLockfileResolutions(lockfile, [verifier])
  expect(seen).toEqual([{ name: 'react', version: '18.0.0' }])
})

test('does not collapse same (name, version) with different resolutions', async () => {
  // Two entries sharing a name@version but pinned via different protocols
  // (npm registry vs. a URL-keyed tarball whose snapshot copied the same
  // semver `version` from its manifest). If the dedup key were just
  // `name@version` one would silently overwrite the other and a
  // protocol-scoped verifier would short-circuit on the survivor —
  // letting the real entry skip the gate.
  const npmResolution = tarballResolution('sha512-a')
  const directTarballResolution = { integrity: 'sha512-b', tarball: 'https://example.com/foo.tgz' }
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: npmResolution },
    'foo@https://example.com/foo.tgz': { resolution: directTarballResolution, version: '1.0.0' },
  })
  const seenResolutions: unknown[] = []
  const verifier = wrap(async (resolution) => {
    seenResolutions.push(resolution)
    return { ok: true }
  })

  await verifyLockfileResolutions(lockfile, [verifier])
  expect(seenResolutions).toEqual(expect.arrayContaining([npmResolution, directTarballResolution]))
  expect(seenResolutions).toHaveLength(2)
})

test('the verifier sees the resolution shape verbatim', async () => {
  const npmResolution = tarballResolution()
  const gitResolution = { type: 'git', repo: 'https://example.com/git-pkg.git', commit: 'abc' }
  const lockfile = makeLockfile({
    'npm-pkg@1.0.0': { resolution: npmResolution },
    'git-pkg@git+https://example.com/git-pkg.git#abc': { resolution: gitResolution, version: '1.0.0' },
  })
  const received: unknown[] = []
  const verifier = wrap(async (resolution) => {
    received.push(resolution)
    return { ok: true }
  })

  await verifyLockfileResolutions(lockfile, [verifier])
  expect(received).toEqual(expect.arrayContaining([npmResolution, gitResolution]))
})

test('keeps the per-policy code when every violation in the batch shares it', async () => {
  // Same code across all violations → throw with that code so existing
  // handlers / docs / search routes still match. Mixed-code coverage is
  // in the dedicated "throws a generic code ..." test above.
  const lockfile = makeLockfile({
    'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    'b@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const verifier = wrap(async () => ({
    ok: false,
    code: 'POLICY_A',
    reason: 'failed',
  }))

  await expect(verifyLockfileResolutions(lockfile, [verifier])).rejects.toMatchObject({
    code: 'ERR_PNPM_POLICY_A',
  })
})

test('runs every active verifier per entry and stops at the first failure', async () => {
  const lockfile = makeLockfile({
    'a@1.0.0': { resolution: tarballResolution('sha512-a') },
  })
  const calls: string[] = []
  const firstOk = wrap(async () => {
    calls.push('first')
    return { ok: true }
  }, NOOP_SLOT)
  const secondFail = wrap(async () => {
    calls.push('second')
    return { ok: false, code: 'SECOND_POLICY', reason: 'nope' }
  }, NOOP_SLOT)

  await expect(verifyLockfileResolutions(lockfile, [firstOk, secondFail]))
    .rejects.toMatchObject({ code: 'ERR_PNPM_SECOND_POLICY' })
  // Both verifiers ran on the entry; ordering follows the list.
  expect(calls).toEqual(['first', 'second'])
})

function exampleSlot (current: number): Omit<ResolutionVerifier, 'verify'> {
  return {
    policy: { minimumReleaseAge: current },
    canTrustPastCheck: (cached) => {
      const past = cached.minimumReleaseAge
      return typeof past === 'number' && past >= current
    },
  }
}

test('skips the verifier when the cache holds an unchanged lockfile + matching policy', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const lockfile = makeLockfile({
      'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    })

    let calls = 0
    const counting = wrap(async () => {
      calls++
      return { ok: true }
    }, exampleSlot(60))

    // First call has no cache record yet — verifier runs.
    await verifyLockfileResolutions(lockfile, [counting], {
      cacheDir, lockfilePath,
    })
    expect(calls).toBe(1)

    // Second call against the same lockfile + policy — cache short-circuit.
    await verifyLockfileResolutions(lockfile, [counting], {
      cacheDir, lockfilePath,
    })
    expect(calls).toBe(1)
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('emits a cached event when the cache short-circuits verification', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const lockfile = makeLockfile({
      'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    })
    const verifier = wrap(async () => ({ ok: true }), exampleSlot(60))

    await verifyLockfileResolutions(lockfile, [verifier], { cacheDir, lockfilePath })

    const debugSpy = jest.spyOn(lockfileVerificationLogger, 'debug')
    try {
      await verifyLockfileResolutions(lockfile, [verifier], { cacheDir, lockfilePath })
      expect(debugSpy.mock.calls.map(([message]) => message)).toEqual([
        {
          status: 'cached',
          verifiedAt: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T/),
          lockfilePath,
        },
      ])
    } finally {
      debugSpy.mockRestore()
    }
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('reuses the cached verdict silently when no policy verifiers are active', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const lockfile = makeLockfile({
      'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    })

    await verifyLockfileResolutions(lockfile, [wrap(async () => ({ ok: true }), exampleSlot(60))], {
      cacheDir, lockfilePath,
    })

    const debugSpy = jest.spyOn(lockfileVerificationLogger, 'debug')
    try {
      await verifyLockfileResolutions(lockfile, [], { cacheDir, lockfilePath })
      expect(debugSpy).not.toHaveBeenCalled()
    } finally {
      debugSpy.mockRestore()
    }
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('does not write a cache record when verification rejects', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const lockfile = makeLockfile({
      'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    })

    const rejecting = wrap(async () => ({
      ok: false,
      code: 'POLICY_X',
      reason: 'failed',
    }), exampleSlot(60))

    await expect(
      verifyLockfileResolutions(lockfile, [rejecting], {
        cacheDir, lockfilePath,
      })
    ).rejects.toThrow()

    // No record was written — a rejecting verification must rerun next install.
    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    await expect(fs.promises.access(cacheFile)).rejects.toThrow()
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

const isA100 = (name: string, version: string): boolean => name === 'a' && version === '1.0.0'

test('re-resolved entries skip the policy verifiers', async () => {
  const lockfile = makeLockfile({
    'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    'b@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const rejecting = wrap(async () => ({
    ok: false,
    code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    reason: 'version not present in registry manifest',
  }))

  const error = await verifyLockfileResolutions(lockfile, [rejecting], { isReplaced: isA100 }).catch((err: unknown) => err)

  expect(error).toMatchObject({ message: expect.stringContaining('b@1.0.0 version not present') })
  expect(error).not.toMatchObject({ message: expect.stringContaining('a@1.0.0') })
})

test('does not write a cache record when re-resolved entries were skipped', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const lockfile = makeLockfile({
      'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    })
    const rejecting = wrap(async () => ({
      ok: false,
      code: 'POLICY_X',
      reason: 'failed',
    }), exampleSlot(60))

    await verifyLockfileResolutions(lockfile, [rejecting], { cacheDir, lockfilePath, isReplaced: isA100 })

    await expect(
      verifyLockfileResolutions(lockfile, [rejecting], { cacheDir, lockfilePath })
    ).rejects.toThrow()
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('rejects a registry-style depPath backed by a git resolution, even with no verifiers', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'git', repo: 'https://example.com/foo.git', commit: 'abc123' } },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    hint: expect.not.stringMatching(/relax the policy/),
    message: expect.stringMatching(/foo@1\.0\.0/),
  })
})

test('rejects a registry-style depPath backed by a git-hosted tarball resolution', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: `https://codeload.github.com/org/foo/tar.gz/${GIT_COMMIT}`, gitHosted: true } },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('rejects a registry-style depPath backed by a directory resolution', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'directory', directory: '../foo' } },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('accepts registry-style depPaths with registry and all-registry variations resolutions', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: tarballResolution() },
    'bar@1.0.0': {
      resolution: {
        type: 'variations',
        variants: [
          { targets: [{ os: 'darwin' }], resolution: tarballResolution('sha512-a') },
          { targets: [{ os: 'linux' }], resolution: tarballResolution('sha512-b') },
        ],
      },
    },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('rejects a registry-style depPath whose variations resolution hides a git variant', async () => {
  const lockfile = makeLockfile({
    'bar@1.0.0': {
      resolution: {
        type: 'variations',
        variants: [
          { targets: [{ os: 'darwin' }], resolution: tarballResolution('sha512-a') },
          { targets: [{ os: 'linux' }], resolution: { type: 'git', repo: 'https://example.com/bar.git', commit: 'abc123' } },
        ],
      },
    },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('the npm verifier rejects a variations resolution whose inner tarball is not the registry tarball', async () => {
  prepareEmpty()
  const { resolutionVerifiers } = testDefaults()
  const integrity = getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  const lockfile = makeLockfile({
    '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': {
      resolution: {
        type: 'variations',
        variants: [
          {
            targets: [{ os: 'darwin' }],
            resolution: {
              integrity,
              tarball: `http://localhost:${REGISTRY_MOCK_PORT}/npm/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
            },
          },
          {
            targets: [{ os: 'linux' }],
            resolution: { integrity, tarball: 'https://evil.example/dep-of-pkg-with-1-dep-100.0.0.tgz' },
          },
        ],
      },
    },
  })
  await expect(verifyLockfileResolutions(lockfile, resolutionVerifiers)).rejects.toMatchObject({
    code: 'ERR_PNPM_TARBALL_URL_MISMATCH',
    message: expect.stringContaining('@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'),
  })
})

test('does not flag artifact depPaths with non-registry resolutions', async () => {
  const lockfile = makeLockfile({
    'foo@git+https://example.com/foo.git#abc123': { resolution: { type: 'git', repo: 'https://example.com/foo.git', commit: 'abc123' }, version: '1.0.0' },
    'bar@https://example.com/bar.tgz': { resolution: { integrity: 'sha512-deadbeef', tarball: 'https://example.com/bar.tgz' }, version: '1.0.0' },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('rejects a registry-style depPath whose git-host tarball clears the gitHosted flag', async () => {
  // A tampered lockfile sets a non-truthy gitHosted on a codeload URL to
  // dodge a flag-only check. The URL itself must still flag it.
  for (const gitHosted of [false, 'true', 'false', 0, 1]) {
    const lockfile = makeLockfile({
      'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: `https://codeload.github.com/org/foo/tar.gz/${GIT_COMMIT}`, gitHosted } as never },
    })
    // eslint-disable-next-line no-await-in-loop -- one case at a time keeps a failure tied to its input
    await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
      code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    })
  }
})

test('rejects a registry-style depPath with a non-boolean gitHosted flag', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz', gitHosted: 'true' } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('accepts a registry-style depPath backed by a custom resolver resolution', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'custom:cdn', source: 'foo' } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('rejects a registry-style depPath whose variations resolution has a variant without a resolution', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'variations', variants: [{ targets: [{ os: 'linux' }] }] } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('rejects a registry-style depPath backed by an empty variations resolution', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'variations', variants: [] } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test('rejects a registry-style depPath backed by a non-http(s) tarball URL', async () => {
  // The npm verifier skips non-HTTP and non-HTTPS tarballs, so a file: artifact under a
  // semver key would be trusted with no tarball-URL binding to catch it.
  for (const tarball of ['file:///tmp/evil.tgz', 'ftp://example.com/evil.tgz']) {
    const lockfile = makeLockfile({
      'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball } as never },
    })
    // eslint-disable-next-line no-await-in-loop -- one case at a time keeps a failure tied to its input
    await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
      code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
    })
  }
})

test('accepts a registry-style depPath whose tarball is an http(s) registry URL', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: 'https://registry.npmjs.org/foo/-/foo-1.0.0.tgz' } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('rejects a registry-style depPath whose git-host tarball varies the host casing', async () => {
  // Hostnames are case-insensitive; an upper-case codeload host paired with
  // gitHosted: false must not pass as registry-shaped.
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball: `https://CODELOAD.GITHUB.COM/org/foo/tar.gz/${GIT_COMMIT}`, gitHosted: false } as never },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
  })
})

test.each([
  '../../../escape',
  '@scope/../../escape',
  '.bin',
  '.pnpm',
  'node_modules',
])('rejects an importer dependency alias %p, even with no verifiers', async (alias) => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { [alias]: '1.0.0' },
        dependencies: { [alias]: '1.0.0' },
      },
    },
    packages: {
      'real@1.0.0': { resolution: tarballResolution() },
    },
  } as unknown as LockfileObject
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
    message: expect.stringMatching(/not valid package names/),
  })
})

test('rejects an invalid alias nested in a package snapshot, even with no verifiers', async () => {
  const lockfile = makeLockfile({
    'real@1.0.0': {
      resolution: tarballResolution(),
      dependencies: { '../../../escape': '1.0.0' },
    } as never,
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME',
  })
})

test('accepts valid scoped and unscoped dependency aliases', async () => {
  const lockfile = {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { foo: '1.0.0', '@scope/bar': '1.0.0' },
        dependencies: { foo: '1.0.0', '@scope/bar': '1.0.0' },
      },
    },
    packages: {
      'foo@1.0.0': { resolution: tarballResolution() },
      '@scope/bar@1.0.0': { resolution: tarballResolution() },
    },
  } as unknown as LockfileObject
  await expect(verifyLockfileResolutions(lockfile, [])).resolves.toBeUndefined()
})

test('registry-qualified entries are verified separately, each with its own registry name', async () => {
  // Same name, version, and resolution — only the registry differs. Deduping
  // on anything less than the full identity would verify one and skip the
  // other entirely.
  // Registry-qualified keys only exist in 12.0, so the fixture has to declare
  // that version rather than being a 9.0/12.0 hybrid.
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: tarballResolution() },
    'foo@work:1.0.0': { resolution: tarballResolution() },
    'foo@gh:1.0.0': { resolution: tarballResolution() },
  }, LOCKFILE_VERSION)

  const seen: Array<{ name: string, version: string, registryName?: string }> = []
  const recordingVerifier = wrap(async (_resolution, ctx) => {
    seen.push({ name: ctx.name, version: ctx.version, registryName: ctx.registryName })
    return { ok: true }
  })

  await expect(verifyLockfileResolutions(lockfile, [recordingVerifier])).resolves.toBeUndefined()

  expect(seen).toHaveLength(3)
  // Every candidate reports the bare semver plus its own alias.
  expect(seen.every((entry) => entry.name === 'foo' && entry.version === '1.0.0')).toBe(true)
  expect(new Set(seen.map((entry) => entry.registryName)))
    .toStrictEqual(new Set([undefined, 'work', 'gh']))
})

test('isReplaced distinguishes plain semver keys from registry-qualified keys', async () => {
  const lockfile = makeLockfile({
    '@corp/foo@1.0.0': { resolution: tarballResolution() },
    '@corp/foo@legacy:1.0.0': { resolution: tarballResolution() },
  }, LOCKFILE_VERSION)

  const seen: Array<{ name: string, registryName?: string }> = []
  const recordingVerifier = wrap(async (_resolution, ctx) => {
    seen.push({ name: ctx.name, registryName: ctx.registryName })
    return { ok: true }
  })

  await expect(verifyLockfileResolutions(lockfile, [recordingVerifier], {
    isReplaced: (name, _version, candidate) => name === '@corp/foo' && candidate?.registryName == null,
  })).resolves.toBeUndefined()

  expect(seen).toStrictEqual([{ name: '@corp/foo', registryName: 'legacy' }])
})

test('matchReplacedLockfileEntries covers uninstallSome without pre-uninstall scripts and keeps verification when pre-uninstall scripts run', () => {
  const rootDir = '/workspace' as import('@pnpm/types').ProjectRootDir
  const makeCtx = (scripts?: Record<string, string>) => ({
    existsNonEmptyWantedLockfile: true,
    projects: {
      [rootDir]: {
        id: '.' as import('@pnpm/types').ProjectId,
        manifest: {
          dependencies: { bar: '1.0.0' },
          scripts,
        },
      },
    } as never,
    wantedLockfile: {
      lockfileVersion: LOCKFILE_VERSION,
      importers: { '.': {} },
    } as unknown as LockfileObject,
  })
  const baseOpts = {
    depth: 0,
    fixLockfile: false,
    frozenLockfile: false,
    frozenLockfileIfExists: false,
    ignorePackageManifest: false,
    registriesByPrefix: { legacy: 'https://old.example/' },
    registriesByScope: {
      default: 'https://registry.npmjs.org/',
      '@corp': 'https://new.example/',
    },
    useLockfile: true,
  }
  const uninstallMutation = [{
    mutation: 'uninstallSome' as const,
    dependencyNames: ['bar'],
    rootDir,
  }]

  expect(matchReplacedLockfileEntries(uninstallMutation, makeCtx(), {
    ...baseOpts,
    lockedRegistriesByScope: { '@corp': 'https://new.example/' },
  })).toBeUndefined()

  const isReplaced = matchReplacedLockfileEntries(uninstallMutation, makeCtx(), {
    ...baseOpts,
    lockedRegistriesByScope: { '@corp': 'https://old.example/' },
  })
  expect(isReplaced).toBeDefined()
  expect(isReplaced!('@corp/foo', '1.0.0')).toBe(true)
  expect(isReplaced!('@corp/foo', '1.0.0', { registryName: 'legacy' })).toBe(false)

  expect(matchReplacedLockfileEntries(uninstallMutation, makeCtx({ preuninstall: 'node check.js' }), {
    ...baseOpts,
    lockedRegistriesByScope: { '@corp': 'https://old.example/' },
  })).toBeUndefined()

  expect(matchReplacedLockfileEntries(uninstallMutation, makeCtx({ uninstall: 'node check.js' }), {
    ...baseOpts,
    lockedRegistriesByScope: { '@corp': 'https://old.example/' },
  })).toBeUndefined()

  expect(matchReplacedLockfileEntries(uninstallMutation, makeCtx({ preuninstall: 'node check.js' }), {
    ...baseOpts,
    ignoreScripts: true,
    lockedRegistriesByScope: { '@corp': 'https://old.example/' },
  })).toBeDefined()
})

test('invalidates lockfile-verified.jsonl when default or scoped registry changes', async () => {
  prepareEmpty()
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-reg-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')

    const mockRegistry = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`
    const integrity = getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
    const lockfile = makeLockfile({
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': {
        resolution: {
          integrity,
          tarball: `${mockRegistry}@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0`,
        },
      },
    })

    const initialVerifiers = testDefaults({
      registriesByScope: { default: mockRegistry },
    }).resolutionVerifiers
    await verifyLockfileResolutions(lockfile, initialVerifiers, { cacheDir, lockfilePath })

    // Repeat run with equivalent registry URL spelling hits the cache without touching the network.
    const sameRegistryVerifiers = testDefaults({
      registriesByScope: { default: mockRegistry.replace(/\/$/, '') },
    }).resolutionVerifiers
    const debugSpy = jest.spyOn(lockfileVerificationLogger, 'debug')
    try {
      await verifyLockfileResolutions(lockfile, sameRegistryVerifiers, { cacheDir, lockfilePath })
      expect(debugSpy).toHaveBeenCalledWith(expect.objectContaining({ status: 'cached', lockfilePath }))
    } finally {
      debugSpy.mockRestore()
    }

    // Changing default registry must miss the cache and attempt verification against the new registry.
    const changedDefaultVerifiers = testDefaults(
      { registriesByScope: { default: 'http://127.0.0.1:1/' } },
      undefined,
      { retry: { retries: 0 } }
    ).resolutionVerifiers
    await expect(
      verifyLockfileResolutions(lockfile, changedDefaultVerifiers, { cacheDir, lockfilePath })
    ).rejects.toThrow()

    // Changing @pnpm.e2e scope registry must also miss the cache and re-verify against the scoped registry.
    const changedScopeVerifiers = testDefaults(
      {
        registriesByScope: {
          default: mockRegistry,
          '@pnpm.e2e': 'http://127.0.0.1:1/',
        },
      },
      undefined,
      { retry: { retries: 0 } }
    ).resolutionVerifiers
    await expect(
      verifyLockfileResolutions(lockfile, changedScopeVerifiers, { cacheDir, lockfilePath })
    ).rejects.toThrow()
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

function makeUrlSubdepLockfile (): LockfileObject {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0' },
      },
    },
    packages: {
      'reg-parent@1.0.0': {
        resolution: tarballResolution(),
        dependencies: {
          exotic: 'http://127.0.0.1:41999/exotic.tgz',
        },
      },
      'exotic@http://127.0.0.1:41999/exotic.tgz': {
        resolution: {
          integrity: 'sha512-deadbeef',
          tarball: 'http://127.0.0.1:41999/exotic.tgz',
        },
        version: '0.0.0',
      },
    },
  } as unknown as LockfileObject
}

test('rejects a URL tarball subdependency when blockExoticSubdeps is enabled', async () => {
  const lockfile = makeUrlSubdepLockfile()
  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
    hint: expect.stringMatching(/relax the policy/),
    message: expect.stringContaining('exotic@0.0.0 (resolved via url) is not allowed in subdependencies when blockExoticSubdeps is enabled'),
  })
})

test('allows a URL tarball subdependency when blockExoticSubdeps is disabled', async () => {
  const lockfile = makeUrlSubdepLockfile()
  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: false })).resolves.toBeUndefined()
})

test('rejects git and git-hosted tarball subdependencies when blockExoticSubdeps is enabled', async () => {
  const ghTarball = `https://codeload.github.com/acme/gh-pkg/tar.gz/${GIT_COMMIT}`
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0' },
      },
    },
    packages: {
      'reg-parent@1.0.0': {
        resolution: tarballResolution(),
        dependencies: {
          'git-pkg': 'git+https://example.com/repo.git#deadbeef',
        },
        optionalDependencies: {
          'gh-pkg': ghTarball,
        },
      },
      'git-pkg@git+https://example.com/repo.git#deadbeef': {
        resolution: { type: 'git', repo: 'https://example.com/repo.git', commit: 'deadbeef' },
        version: '1.2.3',
      },
      [`gh-pkg@${ghTarball}`]: {
        resolution: { tarball: ghTarball },
        version: '2.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
    message: expect.stringMatching(/gh-pkg@2\.0\.0 \(resolved via git-repository\)[\s\S]*git-pkg@1\.2\.3 \(resolved via git-repository\)/),
  })
})

test('rejects an exotic subdependency under a direct exotic parent and handles versionless packages', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'direct-git': 'git+https://example.com/direct.git' },
        dependencies: { 'direct-git': 'git+https://example.com/direct.git#11111111' },
      },
    },
    packages: {
      'direct-git@git+https://example.com/direct.git#11111111': {
        resolution: { type: 'git', repo: 'https://example.com/direct.git', commit: '11111111' },
        version: '1.0.0',
        dependencies: {
          'nested-exotic': 'https://example.com/nested.tgz',
        },
      },
      'nested-exotic@https://example.com/nested.tgz': {
        resolution: { tarball: 'https://example.com/nested.tgz' },
      },
    },
  } as unknown as LockfileObject

  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
    message: expect.stringContaining('nested-exotic@https://example.com/nested.tgz (resolved via url)'),
  })
})

test('rejects a tampered file: depPath backed by a remote tarball', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0' },
      },
    },
    packages: {
      'reg-parent@1.0.0': {
        resolution: tarballResolution(),
        dependencies: { sneaky: 'file:local.tgz' },
      },
      'sneaky@file:local.tgz': {
        resolution: { tarball: 'https://evil.example/payload.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })
})

test('allows direct importer, workspace snapshot, peer-provided exotic, local-file, and runtime subdependencies', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: {
          'direct-tarball': 'https://example.com/direct.tgz',
          'ws-injected': 'workspace:*',
          'ext-ws': 'workspace:../ext-ws',
          'react-dom': '18.2.0',
        },
        dependencies: {
          'direct-tarball': 'https://example.com/direct.tgz',
          'ws-injected': 'file:packages/ws-injected',
          'ext-ws': 'file:../ext-ws',
          'react-dom': '18.2.0(direct-tarball@https://example.com/direct.tgz)',
        },
      },
      'packages/ws-injected': {
        specifiers: { 'ws-git': 'git+https://example.com/ws-git.git' },
        dependencies: { 'ws-git': 'git+https://example.com/ws-git.git#22222222' },
      },
    },
    packages: {
      'direct-tarball@https://example.com/direct.tgz': {
        resolution: { tarball: 'https://example.com/direct.tgz', integrity: 'sha512-deadbeef' },
        version: '1.0.0',
      },
      'ws-injected@file:packages/ws-injected': {
        resolution: { type: 'directory', directory: 'packages/ws-injected' },
        dependencies: { 'ws-git': 'git+https://example.com/ws-git.git#22222222' },
      },
      'ext-ws@file:../ext-ws': {
        resolution: { type: 'directory', directory: '../ext-ws' },
        dependencies: { 'ext-tarball': 'https://example.com/ext.tgz' },
      },
      'ws-git@git+https://example.com/ws-git.git#22222222': {
        resolution: { type: 'git', repo: 'https://example.com/ws-git.git', commit: '22222222' },
        version: '1.0.0',
      },
      'ext-tarball@https://example.com/ext.tgz': {
        resolution: { tarball: 'https://example.com/ext.tgz' },
        version: '1.0.0',
      },
      'react-dom@18.2.0(direct-tarball@https://example.com/direct.tgz)': {
        resolution: tarballResolution(),
        peerDependencies: { 'direct-tarball': '*' },
        dependencies: {
          'direct-tarball': 'https://example.com/direct.tgz',
          'local-tgz': 'file:vendor/local.tgz',
          node: 'runtime:22.0.0',
        },
      },
      'local-tgz@file:vendor/local.tgz': {
        resolution: { tarball: 'file:vendor/local.tgz', integrity: 'sha512-deadbeef' },
        version: '1.0.0',
      },
      'node@runtime:22.0.0': {
        resolution: {
          type: 'binary',
          archive: 'tarball',
          bin: 'bin/node',
          integrity: 'sha512-deadbeef',
          url: 'https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
        },
        version: '22.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(lockfile, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: {
        knownImporterIds: new Set(['.', 'packages/ws-injected']),
        extraWorkspaceDirs: ['../ext-ws'],
      },
    })
  ).resolves.toBeUndefined()
})

test('rejects a forged peer suffix when no importer or workspace package provides the exotic package', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0(evil@https://evil.example/payload.tgz)' },
      },
    },
    packages: {
      'reg-parent@1.0.0(evil@https://evil.example/payload.tgz)': {
        resolution: tarballResolution(),
        peerDependencies: { evil: '*' },
        dependencies: { evil: 'https://evil.example/payload.tgz' },
      },
      'evil@https://evil.example/payload.tgz': {
        resolution: { tarball: 'https://evil.example/payload.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })
})

test('invalidates lockfile-verified.jsonl when blockExoticSubdeps is enabled after a prior verification', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-exotic-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, `lockfileVersion: '${LOCKFILE_VERSION}'\n`)
    const lockfile = makeUrlSubdepLockfile()
    const verifier = wrap(async () => ({ ok: true }), exampleSlot(0))

    recordLockfileVerified({
      cacheDir,
      lockfile,
      resolutionVerifiers: [verifier],
      blockExoticSubdeps: false,
    })

    const debugSpy = jest.spyOn(lockfileVerificationLogger, 'debug')
    try {
      await expect(
        verifyLockfileResolutions(lockfile, [verifier], {
          cacheDir,
          lockfilePath,
          blockExoticSubdeps: false,
        })
      ).resolves.toBeUndefined()
      expect(debugSpy).toHaveBeenCalledWith(expect.objectContaining({ status: 'cached', lockfilePath }))
    } finally {
      debugSpy.mockRestore()
    }

    await expect(
      verifyLockfileResolutions(lockfile, [verifier], {
        cacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
      })
    ).rejects.toMatchObject({
      code: 'ERR_PNPM_EXOTIC_SUBDEP',
    })
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('skips replaced exotic subdependency edges and does not cache the lockfile', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-exotic-rep-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, `lockfileVersion: '${LOCKFILE_VERSION}'\n`)
    const lockfile = makeUrlSubdepLockfile()

    await expect(
      verifyLockfileResolutions(lockfile, [], {
        cacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
        isReplaced: (name, version) => name === 'reg-parent' && version === '1.0.0',
      })
    ).resolves.toBeUndefined()

    await expect(
      verifyLockfileResolutions(lockfile, [], {
        cacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
      })
    ).rejects.toMatchObject({
      code: 'ERR_PNPM_EXOTIC_SUBDEP',
    })

    const isolatedCacheDir = path.join(tmpDir, 'isolated-cache')
    let verifierCalls = 0
    const verifier = wrap(async () => {
      verifierCalls += 1
      return { ok: true }
    }, exampleSlot(0))
    let isReplacedCalls = 0
    await expect(
      verifyLockfileResolutions(lockfile, [verifier], {
        cacheDir: isolatedCacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
        isReplaced: () => {
          isReplacedCalls += 1
          return isReplacedCalls === 1
        },
      })
    ).resolves.toBeUndefined()
    expect(verifierCalls).toBe(2)
    expect(fs.existsSync(path.join(isolatedCacheDir, 'lockfile-verified.jsonl'))).toBe(false)
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('rejects phantom lockfile.importers entries outside knownImporterIds', async () => {
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0(evil@https://evil.example/payload.tgz)' },
      },
      'ghost-pkg': {
        specifiers: { evil: 'https://evil.example/payload.tgz' },
        dependencies: { evil: 'https://evil.example/payload.tgz' },
      },
    },
    packages: {
      'reg-parent@1.0.0(evil@https://evil.example/payload.tgz)': {
        resolution: tarballResolution(),
        peerDependencies: { evil: '*' },
        dependencies: { evil: 'https://evil.example/payload.tgz' },
      },
      'evil@https://evil.example/payload.tgz': {
        resolution: { tarball: 'https://evil.example/payload.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(lockfile, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.']) },
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })
})

test('validates runtime binary origins, configured mirrors, aliases, semver, and variations', async () => {
  const makeRuntimeLockfile = (
    alias: string,
    ref: string,
    pkgKey: string,
    resolution: Record<string, unknown>
  ): LockfileObject => ({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0' },
      },
    },
    packages: {
      'reg-parent@1.0.0': {
        resolution: tarballResolution(),
        dependencies: { [alias]: ref },
      },
      [pkgKey]: {
        resolution,
        version: '22.0.0',
      },
    },
  } as unknown as LockfileObject)

  const offOrigin = makeRuntimeLockfile('node', 'runtime:22.0.0', 'node@runtime:22.0.0', {
    type: 'binary',
    archive: 'tarball',
    bin: 'bin/node',
    integrity: 'sha512-deadbeef',
    url: 'https://mirror.corp.example/node/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
  })
  await expect(verifyLockfileResolutions(offOrigin, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })
  await expect(
    verifyLockfileResolutions(offOrigin, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { nodeMirrors: ['https://mirror.corp.example/node'] },
    })
  ).resolves.toBeUndefined()

  const aliased = makeRuntimeLockfile('evil', 'node@runtime:22.0.0', 'node@runtime:22.0.0', {
    type: 'binary',
    archive: 'tarball',
    bin: 'bin/node',
    integrity: 'sha512-deadbeef',
    url: 'https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
  })
  await expect(verifyLockfileResolutions(aliased, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })

  const nonSemver = makeRuntimeLockfile('node', 'runtime:not-semver', 'node@runtime:not-semver', {
    type: 'binary',
    archive: 'tarball',
    bin: 'bin/node',
    integrity: 'sha512-deadbeef',
    url: 'https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz',
  })
  await expect(verifyLockfileResolutions(nonSemver, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })

  const poisonedVariations = makeRuntimeLockfile('node', 'runtime:22.0.0', 'node@runtime:22.0.0', {
    type: 'variations',
    variants: [
      {
        targets: [{ os: 'linux', cpu: 'x64' }],
        resolution: {
          type: 'binary',
          archive: 'tarball',
          bin: 'bin/node',
          integrity: 'sha512-deadbeef',
          url: 'https://evil.example/node.tar.gz',
        },
      },
    ],
  })
  await expect(verifyLockfileResolutions(poisonedVariations, [], { blockExoticSubdeps: true })).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })
})

test('guards Object.prototype property lookups and non-registry isReplaced spoofing', async () => {
  const inheritedRecord = Object.create({ inherited: '^1.0.0' }) as Record<string, unknown>
  expect(ownStringProp(inheritedRecord, 'inherited')).toBeUndefined()
  expect(ownStringProp({ own: '^1.0.0' }, 'own')).toBe('^1.0.0')

  const protoLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0' },
        dependencies: { 'reg-parent': '1.0.0(toString@https://evil.example/p.tgz)' },
      },
    },
    packages: {
      'reg-parent@1.0.0(toString@https://evil.example/p.tgz)': {
        resolution: tarballResolution(),
        dependencies: { toString: 'https://evil.example/p.tgz' },
      },
      'toString@https://evil.example/p.tgz': {
        resolution: { tarball: 'https://evil.example/p.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(protoLockfile, [], {
      blockExoticSubdeps: true,
      isReplaced: (name, version) => name === 'toString' && version === '1.0.0',
    })
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_EXOTIC_SUBDEP',
  })

  const makeProvidedPeerLockfile = (peerDependencies: Record<string, string>): LockfileObject => ({
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'reg-parent': '1.0.0', toString: 'https://evil.example/p.tgz' },
        dependencies: {
          'reg-parent': '1.0.0(toString@https://evil.example/p.tgz)',
          toString: 'https://evil.example/p.tgz',
        },
      },
    },
    packages: {
      'reg-parent@1.0.0(toString@https://evil.example/p.tgz)': {
        resolution: tarballResolution(),
        peerDependencies,
        dependencies: { toString: 'https://evil.example/p.tgz' },
      },
      'toString@https://evil.example/p.tgz': {
        resolution: { tarball: 'https://evil.example/p.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject)

  const inheritedPeers = Object.create({ toString: '*' }) as Record<string, string>
  await expect(
    verifyLockfileResolutions(makeProvidedPeerLockfile(inheritedPeers), [], { blockExoticSubdeps: true })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
  await expect(
    verifyLockfileResolutions(makeProvidedPeerLockfile({ toString: '*' }), [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()
})

test('invalidates lockfile-verified.jsonl when exoticSubdepsScope narrows', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-vlr-exotic-scope-'))
  try {
    const cacheDir = path.join(tmpDir, 'cache')
    const lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
    await fs.promises.writeFile(lockfilePath, `lockfileVersion: '${LOCKFILE_VERSION}'\n`)
    const lockfile = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        '.': {
          specifiers: { 'reg-parent': '1.0.0' },
          dependencies: { 'reg-parent': '1.0.0(evil@https://evil.example/p.tgz)' },
        },
        'pkg-b': {
          specifiers: { evil: 'https://evil.example/p.tgz' },
          dependencies: { evil: 'https://evil.example/p.tgz' },
        },
      },
      packages: {
        'reg-parent@1.0.0(evil@https://evil.example/p.tgz)': {
          resolution: tarballResolution(),
          peerDependencies: { evil: '*' },
          dependencies: { evil: 'https://evil.example/p.tgz' },
        },
        'evil@https://evil.example/p.tgz': {
          resolution: { tarball: 'https://evil.example/p.tgz' },
          version: '1.0.0',
        },
      },
    } as unknown as LockfileObject

    const broadScope = { knownImporterIds: new Set(['.', 'pkg-b']) }
    await expect(
      verifyLockfileResolutions(lockfile, [], {
        cacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
        exoticSubdepsScope: broadScope,
      })
    ).resolves.toBeUndefined()

    const debugSpy = jest.spyOn(lockfileVerificationLogger, 'debug')
    try {
      await expect(
        verifyLockfileResolutions(lockfile, [], {
          cacheDir,
          lockfilePath,
          blockExoticSubdeps: true,
          exoticSubdepsScope: broadScope,
        })
      ).resolves.toBeUndefined()
      expect(debugSpy).toHaveBeenCalledWith(expect.objectContaining({ status: 'cached', lockfilePath }))
    } finally {
      debugSpy.mockRestore()
    }

    await expect(
      verifyLockfileResolutions(lockfile, [], {
        cacheDir,
        lockfilePath,
        blockExoticSubdeps: true,
        exoticSubdepsScope: { knownImporterIds: new Set(['.']) },
      })
    ).rejects.toMatchObject({
      code: 'ERR_PNPM_EXOTIC_SUBDEP',
    })
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('rejects non-canonical importer keys, unverified lockfile publishDirectory/workspace targets, encoded runtime escapes, and peer suffix spoofing', async () => {
  await Promise.all(['./', 'ghost/..'].map(async (forgedKey) => {
    const forgedImporter = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        '.': {
          specifiers: { 'reg-parent': '1.0.0' },
          dependencies: { 'reg-parent': '1.0.0(evil@https://evil.example/p.tgz)' },
        },
        [forgedKey]: {
          specifiers: { evil: 'https://evil.example/p.tgz' },
          dependencies: { evil: 'https://evil.example/p.tgz' },
        },
      },
      packages: {
        'reg-parent@1.0.0(evil@https://evil.example/p.tgz)': {
          resolution: tarballResolution(),
          peerDependencies: { evil: '*' },
          dependencies: { evil: 'https://evil.example/p.tgz' },
        },
        'evil@https://evil.example/p.tgz': {
          resolution: { tarball: 'https://evil.example/p.tgz' },
          version: '1.0.0',
        },
      },
    } as unknown as LockfileObject
    await expect(
      verifyLockfileResolutions(forgedImporter, [], {
        blockExoticSubdeps: true,
        exoticSubdepsScope: { knownImporterIds: new Set(['.']) },
      })
    ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
  }))

  const unverifiedPublishDir = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': { publishDirectory: 'packages/forged-pub' },
    },
    packages: {
      'forged-pub@file:packages/forged-pub': {
        resolution: { type: 'directory', directory: 'packages/forged-pub' },
        dependencies: { evil: 'https://evil.example/pub.tgz' },
      },
      'evil@https://evil.example/pub.tgz': {
        resolution: { tarball: 'https://evil.example/pub.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(unverifiedPublishDir, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.']) },
    })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })

  const unverifiedWorkspaceLink = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'forged-ws': 'workspace:*' },
        dependencies: { 'forged-ws': 'link:packages/forged-ws' },
      },
    },
    packages: {
      'forged-ws@file:packages/forged-ws': {
        resolution: { type: 'directory', directory: 'packages/forged-ws' },
        dependencies: { evil2: 'https://evil.example/ws.tgz' },
      },
      'evil2@https://evil.example/ws.tgz': {
        resolution: { tarball: 'https://evil.example/ws.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(unverifiedWorkspaceLink, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.']) },
    })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })

  const badRuntimeUrls = [
    'https://github.com/oven-sh/bun/releases/download/%2e%2e/%2e%2e/evil/repo/releases/download/bun-v1.1.0/bun-linux-x64.zip',
    'https://github.com/oven-sh/bun/releases/download/bun-v1.1.0%2f..%2fevil/bun-linux-x64.zip',
    'https://github.com/oven-sh/bun/releases/download/bun-v1.2.0/bun-linux-x64.zip',
    'https://user@github.com/oven-sh/bun/releases/download/bun-v1.1.0/bun-linux-x64.zip',
  ]
  await Promise.all(badRuntimeUrls.map(async (url) => {
    const runtimeLockfile = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        '.': { specifiers: { 'reg-parent': '1.0.0' }, dependencies: { 'reg-parent': '1.0.0' } },
      },
      packages: {
        'reg-parent@1.0.0': {
          resolution: tarballResolution(),
          dependencies: { bun: 'runtime:1.1.0' },
        },
        'bun@runtime:1.1.0': {
          resolution: { type: 'binary', archive: 'zip', bin: 'bun', integrity: 'sha512-deadbeef', url },
          version: '1.1.0',
        },
      },
    } as unknown as LockfileObject
    await expect(
      verifyLockfileResolutions(runtimeLockfile, [], { blockExoticSubdeps: true })
    ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
  }))

  const forgedHashLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'direct-git': 'git+https://example.com/r.git', 'reg-parent': '1.0.0' },
        dependencies: {
          'direct-git': 'git+https://example.com/r.git#11111111',
          'reg-parent': '1.0.0(0123456789abcdef0123456789abcdef)',
        },
      },
    },
    packages: {
      'reg-parent@1.0.0(0123456789abcdef0123456789abcdef)': {
        resolution: tarballResolution(),
        peerDependencies: { 'direct-git': '*' },
        dependencies: { 'direct-git': 'git+https://example.com/r.git#11111111' },
      },
      'direct-git@git+https://example.com/r.git#11111111': {
        resolution: { type: 'git', repo: 'https://example.com/r.git', commit: '11111111' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(forgedHashLockfile, [], { blockExoticSubdeps: true })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })

  const genuineHash = createPeerDepGraphHash(['direct-git@git+https://example.com/r.git#11111111'], 0)
  const genuineHashLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'direct-git': 'git+https://example.com/r.git', 'reg-parent': '1.0.0' },
        dependencies: {
          'direct-git': 'git+https://example.com/r.git#11111111',
          'reg-parent': `1.0.0${genuineHash}`,
        },
      },
    },
    packages: {
      [`reg-parent@1.0.0${genuineHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: { 'direct-git': '*' },
        dependencies: { 'direct-git': 'git+https://example.com/r.git#11111111' },
      },
      'direct-git@git+https://example.com/r.git#11111111': {
        resolution: { type: 'git', repo: 'https://example.com/r.git', commit: '11111111' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(genuineHashLockfile, [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()

  const mismatchedPeerVariant = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'other-peer': '1.0.0', 'direct-git': 'git+https://example.com/r.git', 'reg-parent': '1.0.0' },
        dependencies: {
          'other-peer': '1.0.0',
          'direct-git': 'git+https://example.com/r.git#11111111(other-peer@1.0.0)',
          'reg-parent': '1.0.0(direct-git@git+https://example.com/r.git#11111111(evil-peer@2.0.0))',
        },
      },
    },
    packages: {
      'other-peer@1.0.0': { resolution: tarballResolution() },
      'evil-peer@2.0.0': { resolution: tarballResolution() },
      'direct-git@git+https://example.com/r.git#11111111(other-peer@1.0.0)': {
        resolution: { type: 'git', repo: 'https://example.com/r.git', commit: '11111111' },
        version: '1.0.0',
        peerDependencies: { 'other-peer': '*' },
        dependencies: { 'other-peer': '1.0.0' },
      },
      'direct-git@git+https://example.com/r.git#11111111(evil-peer@2.0.0)': {
        resolution: { type: 'git', repo: 'https://example.com/r.git', commit: '11111111' },
        version: '1.0.0',
        peerDependencies: { 'other-peer': '*' },
        dependencies: { 'other-peer': 'evil-peer@2.0.0' },
      },
      'reg-parent@1.0.0(direct-git@git+https://example.com/r.git#11111111(evil-peer@2.0.0))': {
        resolution: tarballResolution(),
        peerDependencies: { 'direct-git': '*' },
        dependencies: { 'direct-git': 'git+https://example.com/r.git#11111111(evil-peer@2.0.0)' },
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(mismatchedPeerVariant, [], { blockExoticSubdeps: true })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })

  expect(joinPosixRelPath('C:/workspace', '../../shared')).toBe('C:/shared')
  expect(
    computeExoticSubdepsScopeDigest({ knownImporterIds: new Set(['a\0extra_dirs']) })
  ).not.toBe(
    computeExoticSubdepsScopeDigest({ knownImporterIds: new Set(['a']), extraWorkspaceDirs: ['extra_dirs'] })
  )
  const weaker = snapshotVerificationPolicy([], { blockExoticSubdeps: false })
  expect(canTrustPreparedPolicy(weaker, [], { blockExoticSubdeps: true })).toBe(false)
})

test('blockExoticSubdeps rejects directory resolutions containing backslashes', async () => {
  const backslashDirLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': { specifiers: { 'evil-dir': 'file:packages\\evil' }, dependencies: { 'evil-dir': 'file:packages\\evil' } },
    },
    packages: {
      'evil-dir@file:packages\\evil': {
        resolution: { type: 'directory', directory: 'packages\\evil' },
        dependencies: { evil: 'https://evil.example/backslash.tgz' },
      },
      'evil@https://evil.example/backslash.tgz': {
        resolution: { tarball: 'https://evil.example/backslash.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(backslashDirLockfile, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.']), extraWorkspaceDirs: ['packages/evil'] },
    })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
})

test('buildExoticSubdepsScopeFromContext trusts explicit workspace paths and ignores scoped workspace aliases', () => {
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-exotic-scope-'))
  try {
    fs.mkdirSync(path.join(tmpRoot, '@scope', 'pkg'), { recursive: true })
    fs.writeFileSync(path.join(tmpRoot, '@scope', 'pkg', 'package.json'), JSON.stringify({ name: '@scope/pkg', version: '1.0.0' }))
    fs.mkdirSync(path.join(tmpRoot, 'local-subpkg'), { recursive: true })
    fs.writeFileSync(path.join(tmpRoot, 'local-subpkg', 'package.json'), JSON.stringify({ name: 'local-subpkg', version: '1.0.0' }))

    const scope = buildExoticSubdepsScopeFromContext({
      lockfileDir: tmpRoot,
      projects: {
        '.': {
          id: '.' as never,
          rootDir: tmpRoot as never,
          manifest: {
            name: 'root-app',
            dependencies: {
              'aliased-bare': 'workspace:@scope/pkg',
              'aliased-range': 'workspace:@scope/pkg@^1.0.0',
              'explicit-path': 'workspace:./local-subpkg',
            },
          },
        } as never,
      },
    }, { blockExoticSubdeps: true })
    expect(scope.extraWorkspaceDirs).toContain('local-subpkg')
    expect(scope.extraWorkspaceDirs?.some((dir) => dir.includes('@scope/pkg'))).toBe(false)

    const disabledScope = buildExoticSubdepsScopeFromContext({
      lockfileDir: tmpRoot,
      projects: {
        '.': {
          id: '.' as never,
          rootDir: tmpRoot as never,
          manifest: { publishConfig: { directory: 'unreadable-or-unused' } },
        } as never,
      },
    }, { blockExoticSubdeps: false })
    expect(disabledScope).toStrictEqual({})
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true })
  }
})

test('blockExoticSubdeps recovers hashed peer providers for link: workspace peers and wide leaf snapshots', async () => {
  const linkAndTarballHash = createPeerDepGraphHash([
    { name: 'ws-peer', version: 'packages+ws-peer' },
    'tarball-peer@https://example.com/tarball-peer.tgz',
  ], 0)
  const linkPeerLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'ws-peer': 'workspace:*', 'tarball-peer': 'https://example.com/tarball-peer.tgz', 'reg-parent': '1.0.0' },
        dependencies: {
          'ws-peer': 'link:packages/ws-peer',
          'tarball-peer': 'https://example.com/tarball-peer.tgz',
          'reg-parent': `1.0.0${linkAndTarballHash}`,
        },
      },
      'packages/ws-peer': { specifiers: {} },
    },
    packages: {
      [`reg-parent@1.0.0${linkAndTarballHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: { 'ws-peer': '*', 'tarball-peer': '*' },
        dependencies: {
          'ws-peer': 'link:packages/ws-peer',
          'tarball-peer': 'https://example.com/tarball-peer.tgz',
        },
      },
      'tarball-peer@https://example.com/tarball-peer.tgz': {
        resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
        version: '1.0.0',
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(linkPeerLockfile, [], {
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.', 'packages/ws-peer']) },
    })
  ).resolves.toBeUndefined()

  const transitiveHash = createPeerDepGraphHash([
    'tarball-peer@https://example.com/tarball-peer.tgz',
    'trans-peer@1.0.0',
  ], 0)
  await Promise.all([2, 1050].map(async (leafCount) => {
    const parentDeps: Record<string, string> = {
      'tarball-peer': 'https://example.com/tarball-peer.tgz',
      carrier: '1.0.0',
    }
    const extraPkgs: Record<string, unknown> = {}
    for (let idx = 0; idx < leafCount; idx++) {
      parentDeps[`leaf-${idx}`] = '1.0.0'
      extraPkgs[`leaf-${idx}@1.0.0`] = { resolution: tarballResolution() }
    }
    const wideLockfile = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        '.': {
          specifiers: { 'tarball-peer': 'https://example.com/tarball-peer.tgz', 'reg-parent': '1.0.0' },
          dependencies: {
            'tarball-peer': 'https://example.com/tarball-peer.tgz',
            'reg-parent': `1.0.0${transitiveHash}`,
          },
        },
      },
      packages: {
        [`reg-parent@1.0.0${transitiveHash}`]: {
          resolution: tarballResolution(),
          peerDependencies: { 'tarball-peer': '*' },
          dependencies: parentDeps,
          transitivePeerDependencies: ['trans-peer'],
        },
        'carrier@1.0.0': {
          resolution: tarballResolution(),
          dependencies: { 'trans-peer': '1.0.0' },
        },
        'trans-peer@1.0.0': { resolution: tarballResolution() },
        'tarball-peer@https://example.com/tarball-peer.tgz': {
          resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
          version: '1.0.0',
        },
        ...extraPkgs,
      },
    } as unknown as LockfileObject
    await expect(
      verifyLockfileResolutions(wideLockfile, [], { blockExoticSubdeps: true })
    ).resolves.toBeUndefined()
  }))
})

test('blockExoticSubdeps recovers hashed peer providers when cycle cuts mix bare and full peer keys', async () => {
  const mixedCyclicHash = createPeerDepGraphHash([
    { name: 'cyclic-a', version: '1.0.0' },
    'tarball-peer@https://example.com/tarball-peer.tgz(sub-peer@1.0.0)',
  ], 0)
  const mixedCyclicLockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: {
          'sub-peer': '1.0.0',
          'cyclic-a': '1.0.0',
          'cyclic-b': '1.0.0',
          'tarball-peer': 'https://example.com/tarball-peer.tgz',
          'reg-parent': '1.0.0',
        },
        dependencies: {
          'sub-peer': '1.0.0',
          'cyclic-a': '1.0.0(cyclic-b@1.0.0)',
          'cyclic-b': '1.0.0(cyclic-a@1.0.0)',
          'tarball-peer': 'https://example.com/tarball-peer.tgz(sub-peer@1.0.0)',
          'reg-parent': `1.0.0${mixedCyclicHash}`,
        },
      },
    },
    packages: {
      'sub-peer@1.0.0': { resolution: tarballResolution(), version: '1.0.0' },
      'cyclic-a@1.0.0(cyclic-b@1.0.0)': {
        resolution: tarballResolution(),
        version: '1.0.0',
        peerDependencies: { 'cyclic-b': '*' },
        dependencies: { 'cyclic-b': '1.0.0(cyclic-a@1.0.0)' },
      },
      'cyclic-b@1.0.0(cyclic-a@1.0.0)': {
        resolution: tarballResolution(),
        version: '1.0.0',
        peerDependencies: { 'cyclic-a': '*' },
        dependencies: { 'cyclic-a': '1.0.0(cyclic-b@1.0.0)' },
      },
      'tarball-peer@https://example.com/tarball-peer.tgz(sub-peer@1.0.0)': {
        resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
        version: '1.0.0',
        peerDependencies: { 'sub-peer': '*' },
        dependencies: { 'sub-peer': '1.0.0' },
      },
      [`reg-parent@1.0.0${mixedCyclicHash}`]: {
        resolution: tarballResolution(),
        version: '1.0.0',
        peerDependencies: { 'cyclic-a': '*', 'tarball-peer': '*' },
        dependencies: {
          'cyclic-a': '1.0.0(cyclic-b@1.0.0)',
          'tarball-peer': 'https://example.com/tarball-peer.tgz(sub-peer@1.0.0)',
        },
      },
    },
  } as unknown as LockfileObject
  await expect(
    verifyLockfileResolutions(mixedCyclicLockfile, [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()
})

test('settlePreparedMutationHooksAndVerification re-verifies hooked lockfiles against blockExoticSubdeps', async () => {
  const cleanL0 = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { '.': { specifiers: { 'reg-parent': '1.0.0' }, dependencies: { 'reg-parent': '1.0.0' } } },
    packages: { 'reg-parent@1.0.0': { resolution: tarballResolution() } },
  } as unknown as LockfileObject
  const exoticL1 = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: { '.': { specifiers: { 'reg-parent': '1.0.0' }, dependencies: { 'reg-parent': '1.0.0' } } },
    packages: {
      'reg-parent@1.0.0': { resolution: tarballResolution(), dependencies: { evil: 'https://evil.example/hook.tgz' } },
      'evil@https://evil.example/hook.tgz': { resolution: { tarball: 'https://evil.example/hook.tgz' }, version: '1.0.0' },
    },
  } as unknown as LockfileObject

  const preparedTempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-prepared-hook-'))
  try {
    const lockfilePath = path.join(preparedTempDir, 'pnpm-lock.yaml')
    fs.writeFileSync(lockfilePath, 'lockfileVersion: 9.0\n')
    recordLockfileVerified({
      cacheDir: preparedTempDir,
      lockfile: cleanL0,
      resolutionVerifiers: [],
      blockExoticSubdeps: true,
      exoticSubdepsScope: { knownImporterIds: new Set(['.']), extraWorkspaceDirs: ['.'] },
    })

    const baseOpts = {
      ...(await testDefaults({ fastUnpack: false })),
      cacheDir: preparedTempDir,
      lockfileDir: preparedTempDir,
      blockExoticSubdeps: true,
      trustLockfile: false,
      resolutionVerifiers: [],
      hooks: {},
      allProjects: [],
    }
    const ctxAddExotic = {
      existsWantedLockfile: true,
      lockfileHadConflicts: false,
      lockfileDir: preparedTempDir,
      wantedLockfile: cleanL0,
      currentLockfile: cleanL0,
      projects: { '.': { id: '.', rootDir: preparedTempDir, manifest: { name: 'app' } } },
    }
    const settledReject = await settlePreparedMutationHooksAndVerification(
      { ctx: ctxAddExotic as never, installsOnly: true, opts: baseOpts as never, projects: [] },
      {
        existsWantedLockfile: true,
        hookedProjects: new Map(),
        hookedWantedLockfile: exoticL1,
        lockfileHadConflicts: false,
        preResolutionRan: true,
        validatedInputLockfile: cleanL0,
        verifiedLockfile: true,
        verifiedPolicySnapshot: snapshotVerificationPolicy([], { blockExoticSubdeps: false }),
      }
    )
    expect(ctxAddExotic.wantedLockfile).toBe(exoticL1)
    await expect(settledReject.verifyLockfilePromise).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })

    const ctxRemoveExotic = {
      ...ctxAddExotic,
      wantedLockfile: exoticL1,
      currentLockfile: exoticL1,
    }
    const settledAllow = await settlePreparedMutationHooksAndVerification(
      { ctx: ctxRemoveExotic as never, installsOnly: true, opts: baseOpts as never, projects: [] },
      {
        existsWantedLockfile: true,
        hookedProjects: new Map(),
        hookedWantedLockfile: cleanL0,
        lockfileHadConflicts: false,
        preResolutionRan: true,
        validatedInputLockfile: exoticL1,
        verifiedLockfile: true,
        verifiedPolicySnapshot: snapshotVerificationPolicy([], { blockExoticSubdeps: false }),
      }
    )
    expect(ctxRemoveExotic.wantedLockfile).toBe(cleanL0)
    await expect(settledAllow.verifyLockfilePromise).resolves.toBeUndefined()

    const staleRemoveCacheDir = path.join(preparedTempDir, 'stale-remove-cache')
    const staleRemoveCtx = {
      ...ctxAddExotic,
      wantedLockfile: structuredClone(exoticL1),
      currentLockfile: structuredClone(exoticL1),
    }
    const staleRemoveOpts = {
      ...baseOpts,
      cacheDir: staleRemoveCacheDir,
      hooks: {
        preResolution: [
          async ({ wantedLockfile }: { wantedLockfile: LockfileObject }) => {
            delete wantedLockfile.packages?.['evil@https://evil.example/hook.tgz' as DepPath]
            if (wantedLockfile.packages?.['reg-parent@1.0.0' as DepPath] != null) {
              delete wantedLockfile.packages['reg-parent@1.0.0' as DepPath].dependencies
            }
          },
        ],
      },
    }
    const settledStaleRemove = await settlePreparedMutationHooksAndVerification(
      { ctx: staleRemoveCtx as never, installsOnly: true, opts: staleRemoveOpts as never, projects: [] },
      {
        existsWantedLockfile: true,
        hookedProjects: new Map(),
        hookedWantedLockfile: cleanL0,
        lockfileHadConflicts: false,
        preResolutionRan: true,
        validatedInputLockfile: cleanL0,
        verifiedLockfile: true,
        verifiedPolicySnapshot: snapshotVerificationPolicy([], { blockExoticSubdeps: true }),
      }
    )
    await expect(settledStaleRemove.verifyLockfilePromise).resolves.toBeUndefined()
    expect(fs.existsSync(path.join(staleRemoveCacheDir, 'lockfile-verified.jsonl'))).toBe(false)

    const staleInjectCtx = {
      ...ctxAddExotic,
      wantedLockfile: structuredClone(cleanL0),
      currentLockfile: structuredClone(cleanL0),
    }
    const staleInjectOpts = {
      ...baseOpts,
      hooks: {
        preResolution: [
          async ({ wantedLockfile }: { wantedLockfile: LockfileObject }) => {
            wantedLockfile.packages = structuredClone(exoticL1.packages)
          },
        ],
      },
    }
    const settledStaleInject = await settlePreparedMutationHooksAndVerification(
      { ctx: staleInjectCtx as never, installsOnly: true, opts: staleInjectOpts as never, projects: [] },
      {
        existsWantedLockfile: true,
        hookedProjects: new Map(),
        hookedWantedLockfile: cleanL0,
        lockfileHadConflicts: false,
        preResolutionRan: true,
        validatedInputLockfile: exoticL1,
        verifiedLockfile: true,
        verifiedPolicySnapshot: snapshotVerificationPolicy([], { blockExoticSubdeps: true }),
      }
    )
    await expect(settledStaleInject.verifyLockfilePromise).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
  } finally {
    fs.rmSync(preparedTempDir, { recursive: true, force: true })
  }
})

test('blockExoticSubdeps recovers hashed peer providers across 10,000 peer slots without stack overflow', async () => {
  const peerDeps: Record<string, string> = { 'tarball-peer': '*' }
  const resolvedDeps: Record<string, string> = { 'tarball-peer': 'https://example.com/tarball-peer.tgz' }
  const extraPackages: Record<string, unknown> = {}
  const hashItems = ['tarball-peer@https://example.com/tarball-peer.tgz']
  for (let idx = 0; idx < 10_000; idx++) {
    const name = `wide-peer-${String(idx).padStart(5, '0')}`
    peerDeps[name] = '*'
    resolvedDeps[name] = '1.0.0'
    extraPackages[`${name}@1.0.0`] = { resolution: tarballResolution() }
    hashItems.push(`${name}@1.0.0`)
  }
  const expectedHash = createPeerDepGraphHash(hashItems, 0)
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'tarball-peer': 'https://example.com/tarball-peer.tgz', 'reg-parent': '1.0.0' },
        dependencies: {
          'tarball-peer': 'https://example.com/tarball-peer.tgz',
          'reg-parent': `1.0.0${expectedHash}`,
        },
      },
    },
    packages: {
      [`reg-parent@1.0.0${expectedHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: peerDeps,
        dependencies: resolvedDeps,
      },
      'tarball-peer@https://example.com/tarball-peer.tgz': {
        resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
        version: '1.0.0',
      },
      ...extraPackages,
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()
})

test('blockExoticSubdeps deduplicates collapsing bare peer variants before spending combination budget', async () => {
  const exoticBare = 'tarball-peer@https://example.com/tarball-peer.tgz'
  const exoticVariant = `${exoticBare}(sub@2.0.0)`
  const hashSegments = ['branch-peer@2.0.0', exoticBare]
  const parentDeps: Record<string, string> = {
    'branch-peer': '1.0.0',
    'tarball-peer': 'https://example.com/tarball-peer.tgz(sub@2.0.0)',
    carrier: '1.0.0',
  }
  const carrierDeps: Record<string, string> = { 'branch-peer': '2.0.0' }
  const transitiveNames = ['branch-peer']
  const extraPackages: Record<string, unknown> = {
    'branch-peer@1.0.0': { resolution: tarballResolution() },
    'branch-peer@2.0.0': { resolution: tarballResolution() },
    'sub@1.0.0': { resolution: tarballResolution() },
    'sub@2.0.0': { resolution: tarballResolution() },
  }
  for (let idx = 1; idx <= 13; idx++) {
    const slotName = `dup-${String(idx).padStart(2, '0')}`
    hashSegments.push(`${slotName}@1.0.0`)
    parentDeps[slotName] = '1.0.0(sub@1.0.0)'
    carrierDeps[slotName] = '1.0.0(sub@2.0.0)'
    transitiveNames.push(slotName)
    extraPackages[`${slotName}@1.0.0(sub@1.0.0)`] = { resolution: tarballResolution() }
    extraPackages[`${slotName}@1.0.0(sub@2.0.0)`] = { resolution: tarballResolution() }
  }
  const expectedHash = createPeerDepGraphHash(hashSegments, 0)
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'tarball-peer': 'https://example.com/tarball-peer.tgz', 'reg-parent': '1.0.0' },
        dependencies: {
          'tarball-peer': 'https://example.com/tarball-peer.tgz(sub@2.0.0)',
          'reg-parent': `1.0.0${expectedHash}`,
        },
      },
    },
    packages: {
      [`reg-parent@1.0.0${expectedHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: { 'branch-peer': '*', 'tarball-peer': '*' },
        dependencies: parentDeps,
        transitivePeerDependencies: transitiveNames,
      },
      'carrier@1.0.0': {
        resolution: tarballResolution(),
        dependencies: carrierDeps,
      },
      [exoticVariant]: {
        resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
        version: '1.0.0',
        peerDependencies: { sub: '*' },
        dependencies: { sub: '2.0.0' },
      },
      ...extraPackages,
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()
})

test('blockExoticSubdeps indexes active transitive peers once across unrelated leaf children and charges link refs', async () => {
  const expectedHash = createPeerDepGraphHash([
    'tarball-peer@https://example.com/tarball-peer.tgz',
    'trans-0999@1.0.0',
  ], 0)
  const transitivePeers = Array.from({ length: 1_000 }, (_, idx) => `trans-${String(idx).padStart(4, '0')}`)
  const parentDeps: Record<string, string> = {
    'tarball-peer': 'https://example.com/tarball-peer.tgz',
    noisy: '1.0.0',
    carrier: '1.0.0',
  }
  const extraPackages: Record<string, unknown> = {}
  for (let idx = 0; idx < 2_000; idx++) {
    parentDeps[`leaf-${idx}`] = '1.0.0'
    extraPackages[`leaf-${idx}@1.0.0`] = { resolution: tarballResolution() }
  }
  const lockfile = {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        specifiers: { 'tarball-peer': 'https://example.com/tarball-peer.tgz', 'reg-parent': '1.0.0' },
        dependencies: {
          'tarball-peer': 'https://example.com/tarball-peer.tgz',
          'reg-parent': `1.0.0${expectedHash}`,
        },
      },
    },
    packages: {
      [`reg-parent@1.0.0${expectedHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: { 'tarball-peer': '*' },
        dependencies: parentDeps,
        transitivePeerDependencies: transitivePeers,
      },
      'noisy@1.0.0': {
        resolution: tarballResolution(),
        transitivePeerDependencies: Array.from({ length: 1_000 }, (_, idx) => `unrelated-${idx}`),
      },
      'carrier@1.0.0': {
        resolution: tarballResolution(),
        dependencies: { 'trans-0999': '1.0.0' },
      },
      'trans-0999@1.0.0': { resolution: tarballResolution() },
      'tarball-peer@https://example.com/tarball-peer.tgz': {
        resolution: { tarball: 'https://example.com/tarball-peer.tgz' },
        version: '1.0.0',
      },
      ...extraPackages,
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(lockfile, [], { blockExoticSubdeps: true })
  ).resolves.toBeUndefined()

  const floodedDeps: Record<string, string> = { 'tarball-peer': 'https://example.com/tarball-peer.tgz' }
  for (let idx = 0; idx < 65_537; idx++) {
    floodedDeps[`ws-link-${idx}`] = 'link:packages/ws'
  }
  floodedDeps.carrier = '1.0.0'
  const floodedLockfile = {
    ...lockfile,
    packages: {
      ...lockfile.packages,
      [`reg-parent@1.0.0${expectedHash}`]: {
        resolution: tarballResolution(),
        peerDependencies: { 'tarball-peer': '*' },
        dependencies: floodedDeps,
        transitivePeerDependencies: ['trans-0999'],
      },
    },
  } as unknown as LockfileObject

  await expect(
    verifyLockfileResolutions(floodedLockfile, [], { blockExoticSubdeps: true })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
})

test('buildExoticSubdepsScopeFromContext trusts clean-checkout missing publishConfig.directory and rejects symlinks', () => {
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-clean-publish-dir-'))
  try {
    const cleanLibDir = path.join(tmpRoot, 'packages', 'clean-lib')
    const symlinkLibDir = path.join(tmpRoot, 'packages', 'symlink-lib')
    fs.mkdirSync(cleanLibDir, { recursive: true })
    fs.mkdirSync(symlinkLibDir, { recursive: true })
    if (process.platform !== 'win32') {
      fs.symlinkSync(path.join(tmpRoot, 'missing-outside'), path.join(symlinkLibDir, 'dist'))
    }

    const scope = buildExoticSubdepsScopeFromContext({
      lockfileDir: tmpRoot,
      projects: {
        '.': {
          id: '.' as never,
          rootDir: tmpRoot as never,
          manifest: { name: 'root' },
        } as never,
        'packages/clean-lib': {
          id: 'packages/clean-lib' as never,
          rootDir: cleanLibDir as never,
          manifest: { name: 'clean-lib', publishConfig: { directory: 'dist' } },
        } as never,
        'packages/symlink-lib': {
          id: 'packages/symlink-lib' as never,
          rootDir: symlinkLibDir as never,
          manifest: { name: 'symlink-lib', publishConfig: { directory: 'dist' } },
        } as never,
      },
    }, { blockExoticSubdeps: true })

    expect(scope.extraWorkspaceDirs).toContain('packages/clean-lib/dist')
    if (process.platform !== 'win32') {
      expect(scope.extraWorkspaceDirs).not.toContain('packages/symlink-lib/dist')
    }
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true })
  }
})

test('exotic subdeps scope builders recognize package.yaml and package.json5 manifests', () => {
  const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-alt-manifests-'))
  try {
    const yamlDir = path.join(tmpRoot, 'yaml-pkg')
    const json5Dir = path.join(tmpRoot, 'json5-pkg')
    const emptyDir = path.join(tmpRoot, 'empty-pkg')
    fs.mkdirSync(yamlDir, { recursive: true })
    fs.mkdirSync(json5Dir, { recursive: true })
    fs.mkdirSync(emptyDir, { recursive: true })
    fs.writeFileSync(
      path.join(yamlDir, 'package.yaml'),
      'name: yaml-pkg\nversion: 1.0.0\npublishConfig:\n  directory: dist\n'
    )
    fs.writeFileSync(
      path.join(json5Dir, 'package.json5'),
      "{ name: 'json5-pkg', version: '1.0.0', publishConfig: { directory: 'build' } }\n"
    )

    const ctxScope = buildExoticSubdepsScopeFromContext({
      lockfileDir: tmpRoot,
      projects: {
        '.': {
          id: '.' as never,
          rootDir: tmpRoot as never,
          manifest: {
            name: 'root',
            dependencies: {
              'yaml-pkg': 'workspace:./yaml-pkg',
              'json5-pkg': 'workspace:./json5-pkg',
              'empty-pkg': 'workspace:./empty-pkg',
            },
          },
        } as never,
      },
    }, { blockExoticSubdeps: true })
    expect(ctxScope.extraWorkspaceDirs).toEqual(
      expect.arrayContaining(['yaml-pkg', 'yaml-pkg/dist', 'json5-pkg', 'json5-pkg/build'])
    )
    expect(ctxScope.extraWorkspaceDirs).not.toContain('empty-pkg')

    const dirScope = buildExoticSubdepsScopeFromProjectDirs(
      tmpRoot,
      [yamlDir, json5Dir],
      { blockExoticSubdeps: true }
    )
    expect(dirScope.extraWorkspaceDirs).toEqual(
      expect.arrayContaining(['yaml-pkg', 'yaml-pkg/dist', 'json5-pkg', 'json5-pkg/build'])
    )

    if (process.platform !== 'win32' && process.getuid?.() !== 0) {
      const blockedDir = path.join(tmpRoot, 'blocked-pkg')
      fs.mkdirSync(path.join(blockedDir, 'build'), { recursive: true })
      const blockedPkgJson = path.join(blockedDir, 'package.json')
      fs.writeFileSync(blockedPkgJson, '{"name":"blocked-pkg","version":"1.0.0"}\n')
      fs.writeFileSync(
        path.join(blockedDir, 'package.json5'),
        "{ name: 'blocked-pkg', version: '1.0.0', publishConfig: { directory: 'build' } }\n"
      )
      fs.chmodSync(blockedPkgJson, 0o000)
      try {
        const blockedCtxScope = buildExoticSubdepsScopeFromContext({
          lockfileDir: tmpRoot,
          projects: {
            '.': {
              id: '.' as never,
              rootDir: tmpRoot as never,
              manifest: {
                name: 'root',
                dependencies: { 'blocked-pkg': 'workspace:./blocked-pkg' },
              },
            } as never,
          },
        }, { blockExoticSubdeps: true })
        expect(blockedCtxScope.extraWorkspaceDirs).not.toContain('blocked-pkg')
        expect(blockedCtxScope.extraWorkspaceDirs).not.toContain('blocked-pkg/build')

        const blockedDirScope = buildExoticSubdepsScopeFromProjectDirs(
          tmpRoot,
          [blockedDir],
          { blockExoticSubdeps: true }
        )
        expect(blockedDirScope.extraWorkspaceDirs).not.toContain('blocked-pkg/build')
      } finally {
        fs.chmodSync(blockedPkgJson, 0o644)
      }
    }
  } finally {
    fs.rmSync(tmpRoot, { recursive: true, force: true })
  }
})

test('workspace: path scope trusts real paths including outside lockfileDir, rejects symlinks and node_modules, and caches content-only verdicts', async () => {
  const outerDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-ws-symlink-'))
  try {
    const wsRoot = path.join(outerDir, 'workspace')
    const appDir = path.join(wsRoot, 'packages', 'app')
    const nmPkgDir = path.join(appDir, 'node_modules', 'nm-pkg')
    const sharedDir = path.join(wsRoot, 'packages', 'shared')
    const outsideDir = path.join(outerDir, 'outside-pkg')
    const secretTargetDir = path.join(outerDir, 'secret-target')
    for (const dir of [appDir, nmPkgDir, sharedDir, outsideDir, secretTargetDir]) {
      fs.mkdirSync(dir, { recursive: true })
    }
    fs.writeFileSync(path.join(nmPkgDir, 'package.json'), JSON.stringify({ name: 'nm-pkg', version: '1.0.0' }))
    fs.writeFileSync(path.join(sharedDir, 'package.json'), JSON.stringify({ name: 'shared', version: '1.0.0' }))
    fs.writeFileSync(path.join(outsideDir, 'package.json'), JSON.stringify({ name: 'outside-pkg', version: '1.0.0' }))
    fs.writeFileSync(path.join(secretTargetDir, 'package.json'), JSON.stringify({ name: 'secret-target', version: '1.0.0' }))
    if (process.platform !== 'win32') {
      fs.symlinkSync(secretTargetDir, path.join(wsRoot, 'linked-outside'), 'dir')
    }

    const appManifest = {
      name: 'app',
      version: '1.0.0',
      dependencies: {
        shared: 'workspace:../shared',
        outsidePkg: 'workspace:../../../outside-pkg',
        nmPkg: 'workspace:./node_modules/nm-pkg',
        ...(process.platform !== 'win32' ? { linked: 'workspace:../../linked-outside' } : {}),
      },
    }
    const scope = buildExoticSubdepsScopeFromContext({
      lockfileDir: wsRoot,
      projects: {
        'packages/app': {
          id: 'packages/app' as never,
          rootDir: appDir as never,
          manifest: appManifest,
        } as never,
      },
    }, { blockExoticSubdeps: true })

    expect(scope.extraWorkspaceDirs).toContain('packages/shared')
    expect(scope.extraWorkspaceDirs).toContain('../outside-pkg')
    expect(scope.extraWorkspaceDirs).not.toContain('packages/app/node_modules/nm-pkg')
    expect(scope.extraWorkspaceDirs).not.toContain('linked-outside')

    const externalInjectedWorkspaceLockfile = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        'packages/app': {
          specifiers: { outsidePkg: 'workspace:../../../outside-pkg' },
          dependencies: { outsidePkg: 'file:../outside-pkg' },
        },
      },
      packages: {
        'outside-pkg@file:../outside-pkg': {
          resolution: { type: 'directory', directory: '../outside-pkg' },
          dependencies: { 'exotic-git': 'https://git.example/repo.tar.gz' },
        },
        'exotic-git@https://git.example/repo.tar.gz': {
          resolution: { tarball: 'https://git.example/repo.tar.gz' },
          version: '1.0.0',
        },
      },
    } as unknown as LockfileObject
    await expect(
      verifyLockfileResolutions(externalInjectedWorkspaceLockfile, [], {
        blockExoticSubdeps: true,
        exoticSubdepsScope: scope,
      })
    ).resolves.toBeUndefined()

    if (process.platform !== 'win32') {
      const symlinkSnapshotLockfile = {
        lockfileVersion: LOCKFILE_VERSION,
        importers: {
          'packages/app': {
            specifiers: { linked: 'workspace:../../linked-outside' },
            dependencies: { linked: 'link:../../linked-outside' },
          },
        },
        packages: {
          'linked@file:linked-outside': {
            resolution: { type: 'directory', directory: 'linked-outside' },
            dependencies: { evil: 'https://evil.example/via-symlink.tgz' },
          },
          'evil@https://evil.example/via-symlink.tgz': {
            resolution: { tarball: 'https://evil.example/via-symlink.tgz' },
            version: '1.0.0',
          },
        },
      } as unknown as LockfileObject
      await expect(
        verifyLockfileResolutions(symlinkSnapshotLockfile, [], {
          blockExoticSubdeps: true,
          exoticSubdepsScope: scope,
        })
      ).rejects.toMatchObject({ code: 'ERR_PNPM_EXOTIC_SUBDEP' })
    }

    let verifierCalls = 0
    const countingVerifier = wrap(async () => {
      verifierCalls += 1
      return { ok: true }
    }, exampleSlot(60))
    if (process.platform !== 'win32' && process.getuid?.() !== 0) {
      const secretParent = path.join(sharedDir, 'private')
      fs.mkdirSync(path.join(secretParent, 'dist'), { recursive: true })
      fs.chmodSync(secretParent, 0o000)
      try {
        const unreadableScope = buildExoticSubdepsScopeFromContext({
          lockfileDir: wsRoot,
          projects: {
            'packages/shared': {
              id: 'packages/shared' as never,
              rootDir: sharedDir as never,
              manifest: { name: 'shared', publishConfig: { directory: 'private/dist' } },
            } as never,
          },
        }, { blockExoticSubdeps: true })
        expect(unreadableScope.extraWorkspaceDirs).not.toContain('packages/shared/private/dist')
      } finally {
        fs.chmodSync(secretParent, 0o755)
      }
    }

    const cacheDir = path.join(outerDir, 'content-only-cache')
    const cleanLockfile = makeLockfile({ 'a@1.0.0': { resolution: tarballResolution('sha512-a') } })
    await verifyLockfileResolutions(cleanLockfile, [], { cacheDir })
    expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)

    await verifyLockfileResolutions(cleanLockfile, [countingVerifier], {
      cacheDir,
      blockExoticSubdeps: true,
      exoticSubdepsScope: scope,
    })
    expect(verifierCalls).toBe(1)
    await verifyLockfileResolutions(cleanLockfile, [countingVerifier], {
      cacheDir,
      blockExoticSubdeps: true,
      exoticSubdepsScope: scope,
    })
    expect(verifierCalls).toBe(1)
  } finally {
    fs.rmSync(outerDir, { recursive: true, force: true })
  }
})

test('follows chained workspace: path targets recursively without cycles or non-importer devDependencies', async () => {
  const outerDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-vlr-chain-'))
  try {
    const wsRoot = path.join(outerDir, 'workspace')
    const appDir = path.join(wsRoot, 'packages', 'app')
    const siblingB = path.join(outerDir, 'sibling-b')
    const siblingC = path.join(outerDir, 'sibling-c')
    const devOnlyD = path.join(outerDir, 'dev-only-d')
    for (const dir of [appDir, siblingB, siblingC, devOnlyD]) {
      fs.mkdirSync(dir, { recursive: true })
    }
    fs.writeFileSync(path.join(siblingB, 'package.json'), JSON.stringify({
      name: 'sibling-b',
      version: '1.0.0',
      dependencies: { siblingC: 'workspace:../sibling-c' },
      devDependencies: { devOnlyD: 'workspace:../dev-only-d' },
    }))
    fs.writeFileSync(path.join(siblingC, 'package.json'), JSON.stringify({
      name: 'sibling-c',
      version: '1.0.0',
      dependencies: { siblingB: 'workspace:../sibling-b', exotic: 'https://git.example/repo.tar.gz' },
    }))
    fs.writeFileSync(path.join(devOnlyD, 'package.json'), JSON.stringify({ name: 'dev-only-d', version: '1.0.0' }))

    const scope = buildExoticSubdepsScopeFromContext({
      lockfileDir: wsRoot,
      projects: {
        'packages/app': {
          id: 'packages/app' as never,
          rootDir: appDir as never,
          manifest: { name: 'app', version: '1.0.0', dependencies: { siblingB: 'workspace:../../../sibling-b' } },
        } as never,
      },
    }, { blockExoticSubdeps: true })

    expect(scope.extraWorkspaceDirs).toContain('../sibling-b')
    expect(scope.extraWorkspaceDirs).toContain('../sibling-c')
    expect(scope.extraWorkspaceDirs).not.toContain('../dev-only-d')

    const chainedInjectedLockfile = {
      lockfileVersion: LOCKFILE_VERSION,
      importers: {
        'packages/app': {
          specifiers: { siblingB: 'workspace:../../../sibling-b' },
          dependencies: { siblingB: 'file:../sibling-b' },
        },
      },
      packages: {
        'sibling-b@file:../sibling-b': {
          resolution: { type: 'directory', directory: '../sibling-b' },
          dependencies: { siblingC: 'file:../sibling-c' },
        },
        'sibling-c@file:../sibling-c': {
          resolution: { type: 'directory', directory: '../sibling-c' },
          dependencies: { exotic: 'https://git.example/repo.tar.gz' },
        },
        'exotic@https://git.example/repo.tar.gz': {
          resolution: { tarball: 'https://git.example/repo.tar.gz' },
          version: '1.0.0',
        },
      },
    } as unknown as LockfileObject
    await expect(
      verifyLockfileResolutions(chainedInjectedLockfile, [], { blockExoticSubdeps: true, exoticSubdepsScope: scope })
    ).resolves.toBeUndefined()

    if (process.platform !== 'win32') {
      const realZone = path.join(outerDir, 'real-zone')
      const linkedZone = path.join(outerDir, 'linked-zone')
      const sharedTarget = path.join(linkedZone, 'shared-target')
      const insideApp = path.join(linkedZone, 'inside-app')
      fs.mkdirSync(path.join(realZone, 'shared-target'), { recursive: true })
      fs.mkdirSync(path.join(realZone, 'inside-app'), { recursive: true })
      fs.symlinkSync(realZone, linkedZone, 'dir')
      fs.writeFileSync(path.join(realZone, 'shared-target', 'package.json'), JSON.stringify({ name: 'shared-target', version: '1.0.0' }))
      fs.writeFileSync(path.join(realZone, 'inside-app', 'package.json'), JSON.stringify({ name: 'inside-app', version: '1.0.0' }))

      const crossRootScope = buildExoticSubdepsScopeFromContext({
        lockfileDir: wsRoot,
        projects: {
          'packages/app': {
            id: 'packages/app' as never,
            rootDir: appDir as never,
            manifest: { name: 'app', version: '1.0.0', dependencies: { sharedTarget: `workspace:${sharedTarget}` } },
          } as never,
          '../linked-zone/inside-app': {
            id: '../linked-zone/inside-app' as never,
            rootDir: insideApp as never,
            manifest: { name: 'inside-app', version: '1.0.0', dependencies: { sharedTarget: 'workspace:../shared-target' } },
          } as never,
        },
      }, { blockExoticSubdeps: true })
      expect(crossRootScope.extraWorkspaceDirs).toContain('../linked-zone/shared-target')
    }
  } finally {
    fs.rmSync(outerDir, { recursive: true, force: true })
  }
})
