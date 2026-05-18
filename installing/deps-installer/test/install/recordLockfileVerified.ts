import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { type LockfileObject, readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'

import { recordLockfileVerified } from '../../src/install/recordLockfileVerified.js'
import { tryLockfileVerificationCache } from '../../src/install/verifyLockfileResolutionsCache.js'

let tmpDir!: string
let cacheDir!: string
let lockfileDir!: string

beforeEach(async () => {
  tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-record-verified-'))
  cacheDir = path.join(tmpDir, 'cache')
  lockfileDir = path.join(tmpDir, 'project')
  await fs.promises.mkdir(lockfileDir, { recursive: true })
})

afterEach(async () => {
  await fs.promises.rm(tmpDir, { recursive: true, force: true })
})

function mraVerifier (current: number): ResolutionVerifier {
  return {
    policy: { minimumReleaseAge: current },
    canTrustPastCheck: (cached) => {
      const past = cached.minimumReleaseAge
      return typeof past === 'number' && past >= current
    },
    verify: async () => ({ ok: true }),
  }
}

function makeLockfile (): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      '.': {
        specifiers: { 'is-positive': '^1.0.0' },
        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    packages: {
      'is-positive@1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g=',
          tarball: '',
        },
      },
    },
  } as unknown as LockfileObject
}

test('no-op when cacheDir is undefined', async () => {
  await recordLockfileVerified({
    cacheDir: undefined,
    lockfileDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is empty', async () => {
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: [],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is undefined', async () => {
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: undefined,
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('records nothing when the in-memory lockfile has no packages', async () => {
  await writeWantedLockfile(lockfileDir, makeLockfile())
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile: { lockfileVersion: '9.0', importers: {} } as unknown as LockfileObject,
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)
})

test('records nothing when the lockfile file is missing on disk', async () => {
  // No write — readWantedLockfile returns null and the recording short-circuits
  // instead of indexing a hash the next install won't be able to verify against.
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)
})

test('records the hash of the lockfile as it is parsed back from disk — matches what the next install computes', async () => {
  const writtenLockfile = makeLockfile()
  await writeWantedLockfile(lockfileDir, writtenLockfile)
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile: writtenLockfile,
    resolutionVerifiers: [mraVerifier(60)],
  })

  // Compute the hash the way the next install will: load the lockfile from
  // disk, then `hashObject(loadedLockfile)`. If the recorded hash matches
  // this value, the cache lookup will hit on the next install.
  //
  // This is the round-trip stability guarantee the implementation relies on
  // — it justifies hashing post-load rather than the in-memory write object,
  // since `undefined` vs `{}` differences between the two would otherwise
  // produce diverging hashes under `object-hash`.
  const loaded = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  expect(loaded).not.toBeNull()
  const expectedHash = hashObject(loaded!)

  const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
  const raw = fs.readFileSync(cacheFile, 'utf8').trim()
  const record = JSON.parse(raw) as { lockfile: { hash: string } }
  expect(record.lockfile.hash).toBe(expectedHash)
})

test('records a cache entry that the next install hits — both stat shortcut and hash fallback paths', async () => {
  const lockfile = makeLockfile()
  await writeWantedLockfile(lockfileDir, lockfile)
  await recordLockfileVerified({
    cacheDir,
    lockfileDir,
    lockfile,
    resolutionVerifiers: [mraVerifier(60)],
  })
  const lockfilePath = path.resolve(lockfileDir, WANTED_LOCKFILE)
  const loaded = (await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }))!

  // Stat shortcut: file untouched since record.
  const statResult = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(statResult.hit).toBe(true)

  // Hash fallback: invalidate the recorded inode so the stat shortcut bails,
  // forcing the lookup through hashLockfile. The hash must match for the
  // fallback to hit — this is the cross-machine / CI-checkout path.
  const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
  const cached = JSON.parse(fs.readFileSync(cacheFile, 'utf8').trim()) as {
    lockfile: { inode: string }
  }
  cached.lockfile.inode = '0'
  fs.writeFileSync(cacheFile, `${JSON.stringify(cached)}\n`)

  const hashResult = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(hashResult.hit).toBe(true)
})
