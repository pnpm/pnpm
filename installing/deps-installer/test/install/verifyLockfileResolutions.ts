import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { verifyLockfileResolutions } from '../../src/install/verifyLockfileResolutions.js'

function makeLockfile (packages: Record<string, { resolution: unknown, version?: string }>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: packages as LockfileObject['packages'],
  } as LockfileObject
}

const tarballResolution = (integrity: string = 'sha512-deadbeef') => ({ integrity, tarball: '' })

const okVerifier: ResolutionVerifier = async () => ({ ok: true })

test('no-op when verifyResolution is undefined', async () => {
  const lockfile = makeLockfile({
    'fresh@1.0.0': { resolution: tarballResolution() },
  })
  await expect(verifyLockfileResolutions(lockfile, undefined)).resolves.toBeUndefined()
})

test('no-op when lockfile has no packages', async () => {
  const lockfile = makeLockfile({})
  await expect(verifyLockfileResolutions(lockfile, okVerifier)).resolves.toBeUndefined()
})

test('passes when every entry is verified ok', async () => {
  const lockfile = makeLockfile({
    'lodash@4.17.21': { resolution: tarballResolution() },
    'is-odd@0.1.0': { resolution: tarballResolution() },
  })
  await expect(verifyLockfileResolutions(lockfile, okVerifier)).resolves.toBeUndefined()
})

test('throws with the verifier-supplied code and reason on a single failure', async () => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution() },
  })
  const verifier: ResolutionVerifier = async () => ({
    ok: false,
    code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    reason: 'was published yesterday',
  })

  await expect(verifyLockfileResolutions(lockfile, verifier)).rejects.toMatchObject({
    code: 'ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION',
    message: expect.stringMatching(/is-odd@0\.1\.2 was published yesterday/),
  })
})

test('lists violations in stable order across multiple failures', async () => {
  const lockfile = makeLockfile({
    'fresh-b@2.0.0': { resolution: tarballResolution('sha512-b') },
    'fresh-a@1.0.0': { resolution: tarballResolution('sha512-a') },
  })
  const verifier: ResolutionVerifier = async (_, { name, version }) => ({
    ok: false,
    code: 'POLICY_X',
    reason: `${name}@${version} failed`,
  })

  await expect(verifyLockfileResolutions(lockfile, verifier))
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
  const verifier: ResolutionVerifier = async (_, { name, version }) => ({
    ok: false,
    code: 'POLICY_X',
    reason: `${name}@${version}`,
  })

  await expect(verifyLockfileResolutions(lockfile, verifier))
    .rejects.toThrow(/25 lockfile entries failed verification[\s\S]*…and 5 more/)
})

test('dedupes peer/patch-suffix variants and invokes the verifier once per (name, version)', async () => {
  const lockfile = makeLockfile({
    'react@18.0.0': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(peer-x)': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(patch_hash=abc)(peer-x)': { resolution: tarballResolution('sha512-a') },
  })
  const seen: Array<{ name: string, version: string }> = []
  const verifier: ResolutionVerifier = async (_, { name, version }) => {
    seen.push({ name, version })
    return { ok: true }
  }

  await verifyLockfileResolutions(lockfile, verifier)
  expect(seen).toEqual([{ name: 'react', version: '18.0.0' }])
})

test('the verifier sees the resolution shape verbatim', async () => {
  const npmResolution = tarballResolution()
  const gitResolution = { type: 'git', repo: 'x', commit: 'abc' }
  const lockfile = makeLockfile({
    'npm-pkg@1.0.0': { resolution: npmResolution },
    'git-pkg@1.0.0': { resolution: gitResolution },
  })
  const received: unknown[] = []
  const verifier: ResolutionVerifier = async (resolution) => {
    received.push(resolution)
    return { ok: true }
  }

  await verifyLockfileResolutions(lockfile, verifier)
  expect(received).toEqual(expect.arrayContaining([npmResolution, gitResolution]))
})

test('uses the first violation\'s code when multiple verifiers fire', async () => {
  const lockfile = makeLockfile({
    'a@1.0.0': { resolution: tarballResolution('sha512-a') },
    'b@1.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const verifier: ResolutionVerifier = async (_, { name }) => ({
    ok: false,
    code: name === 'a' ? 'POLICY_A' : 'POLICY_B',
    reason: 'failed',
  })

  await expect(verifyLockfileResolutions(lockfile, verifier)).rejects.toMatchObject({
    code: 'ERR_PNPM_POLICY_A',
  })
})

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
    const counting: ResolutionVerifier = async () => {
      calls++
      return { ok: true }
    }

    // First call has no cache record yet — verifier runs.
    await verifyLockfileResolutions(lockfile, counting, {
      cache: { cacheDir, lockfilePath, minimumReleaseAge: 60 },
    })
    expect(calls).toBe(1)

    // Second call against the same lockfile + policy — cache short-circuit.
    await verifyLockfileResolutions(lockfile, counting, {
      cache: { cacheDir, lockfilePath, minimumReleaseAge: 60 },
    })
    expect(calls).toBe(1)
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

    const rejecting: ResolutionVerifier = async () => ({
      ok: false,
      code: 'POLICY_X',
      reason: 'failed',
    })

    await expect(
      verifyLockfileResolutions(lockfile, rejecting, {
        cache: { cacheDir, lockfilePath, minimumReleaseAge: 60 },
      })
    ).rejects.toThrow()

    // No record was written — a rejecting verification must rerun next install.
    const cacheFile = path.join(cacheDir, 'minimum-release-age-verified.jsonl')
    await expect(fs.promises.access(cacheFile)).rejects.toThrow()
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})
