import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { lockfileVerificationLogger } from '@pnpm/core-loggers'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { verifyLockfileResolutions } from '../../src/install/verifyLockfileResolutions.js'

const GIT_COMMIT = '0123456789abcdef0123456789abcdef01234567'

function makeLockfile (packages: Record<string, { resolution: unknown, version?: string }>): LockfileObject {
  return {
    lockfileVersion: '9.0',
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
  for (let i = 0; i < 25; i++) {
    packages[`pkg-${String(i).padStart(2, '0')}@1.0.0`] = {
      resolution: tarballResolution(`sha512-${i}`),
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
  const gitResolution = { type: 'git', repo: 'x', commit: 'abc' }
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
  // in the dedicated "throws a generic code …" test above.
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

test('rejects a registry-style depPath backed by a git resolution, even with no verifiers', async () => {
  const lockfile = makeLockfile({
    'foo@1.0.0': { resolution: { type: 'git', repo: 'https://example.com/foo.git', commit: 'abc123' } },
  })
  await expect(verifyLockfileResolutions(lockfile, [])).rejects.toMatchObject({
    code: 'ERR_PNPM_RESOLUTION_SHAPE_MISMATCH',
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
    // eslint-disable-next-line no-await-in-loop
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

test('rejects a registry-style depPath backed by a non-http(s) tarball URL', async () => {
  // The npm verifier skips non-http(s) tarballs, so a file: artifact under a
  // semver key would be trusted with no tarball-URL binding to catch it.
  for (const tarball of ['file:///tmp/evil.tgz', 'ftp://example.com/evil.tgz']) {
    const lockfile = makeLockfile({
      'foo@1.0.0': { resolution: { integrity: 'sha512-deadbeef', tarball } as never },
    })
    // eslint-disable-next-line no-await-in-loop
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
