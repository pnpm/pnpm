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
let lockfilePath!: string

beforeEach(async () => {
  tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-record-verified-'))
  cacheDir = path.join(tmpDir, 'cache')
  lockfileDir = path.join(tmpDir, 'project')
  lockfilePath = path.resolve(lockfileDir, WANTED_LOCKFILE)
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

test('no-op when cacheDir is undefined', () => {
  recordLockfileVerified({
    cacheDir: undefined,
    lockfilePath,
    lockfile: makeLockfile(),
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is empty', () => {
  recordLockfileVerified({
    cacheDir,
    lockfilePath,
    lockfile: makeLockfile(),
    resolutionVerifiers: [],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is undefined', () => {
  recordLockfileVerified({
    cacheDir,
    lockfilePath,
    lockfile: makeLockfile(),
    resolutionVerifiers: undefined,
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('records nothing when the in-memory lockfile has no packages', async () => {
  await writeWantedLockfile(lockfileDir, makeLockfile())
  recordLockfileVerified({
    cacheDir,
    lockfilePath,
    lockfile: { lockfileVersion: '9.0', importers: {} } as unknown as LockfileObject,
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)
})

test('records the load-equivalent hash — matches what the next install computes off-disk', async () => {
  // Use a fixture that carries an explicit `undefined` optional field
  // (the real divergence case install-time code produces), then pass
  // the *writer's return value* to recordLockfileVerified — same flow
  // as the production call sites. Passing the in-memory input here
  // instead would silently regress the moment the writer's
  // canonicalization stops matching the reader's output.
  const inMemoryLockfile = {
    ...makeLockfile(),
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
      dedupePeers: undefined,
    },
  } as unknown as LockfileObject
  const written = await writeWantedLockfile(lockfileDir, inMemoryLockfile)
  recordLockfileVerified({
    cacheDir,
    lockfilePath,
    lockfile: written,
    resolutionVerifiers: [mraVerifier(60)],
  })

  // The cache contract: the next install hashes its loaded
  // `LockfileObject` and looks the hash up. The recorded hash must
  // match what that lookup computes.
  const loaded = await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })
  expect(loaded).not.toBeNull()
  const expectedHash = hashObject(loaded!)

  const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
  const record = JSON.parse(fs.readFileSync(cacheFile, 'utf8').trim()) as {
    lockfile: { hash: string, path: string }
  }
  expect(record.lockfile.hash).toBe(expectedHash)
  expect(record.lockfile.path).toBe(lockfilePath)
})

test('respects the caller-supplied lockfilePath — git-branch lockfiles record under their branch-suffixed filename', async () => {
  // Simulates `useGitBranchLockfile`: the actual on-disk lockfile is
  // pnpm-lock.<branch>.yaml, not pnpm-lock.yaml. The helper has no
  // git logic of its own — it records whatever path the caller hands
  // it, so cache lookups on the same path will hit.
  const branchLockfilePath = path.resolve(lockfileDir, 'pnpm-lock.feature-x.yaml')
  await fs.promises.writeFile(branchLockfilePath, 'lockfileVersion: \'9.0\'\n')
  const lockfile = makeLockfile()
  recordLockfileVerified({
    cacheDir,
    lockfilePath: branchLockfilePath,
    lockfile,
    resolutionVerifiers: [mraVerifier(60)],
  })
  const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
  const record = JSON.parse(fs.readFileSync(cacheFile, 'utf8').trim()) as {
    lockfile: { path: string }
  }
  expect(record.lockfile.path).toBe(branchLockfilePath)
})

test('records a cache entry that the next install hits on both the stat shortcut and hash fallback paths', async () => {
  // Mirror real call sites: hand `recordLockfileVerified` the
  // writer's return value rather than the in-memory input. With an
  // explicit `undefined` optional field in the fixture, those two
  // diverge structurally — the in-memory variant would record a hash
  // the next install can't match, and this test would silently miss
  // that regression.
  const inMemoryLockfile = {
    ...makeLockfile(),
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
      dedupePeers: undefined,
    },
  } as unknown as LockfileObject
  const written = await writeWantedLockfile(lockfileDir, inMemoryLockfile)
  recordLockfileVerified({
    cacheDir,
    lockfilePath,
    lockfile: written,
    resolutionVerifiers: [mraVerifier(60)],
  })
  const loaded = (await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }))!

  // Stat shortcut: file untouched since record.
  const statResult = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(statResult.hit).toBe(true)

  // Hash fallback: invalidate stat fields the cache compares against so the
  // shortcut bails. This is the CI-checkout / new-worktree path; the hash
  // has to match for the fallback to hit, which is the whole point of
  // hashing the canonical load-equivalent form. Use `size = -1` (impossible
  // for a real file) rather than zeroing `inode`/`mtimeNs` alone — on
  // Windows `stat.ino` is often 0, which would let the cached record
  // accidentally match and skip the fallback path we want to exercise.
  const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
  const cached = JSON.parse(fs.readFileSync(cacheFile, 'utf8').trim()) as {
    lockfile: { size: number }
  }
  cached.lockfile.size = -1
  fs.writeFileSync(cacheFile, `${JSON.stringify(cached)}\n`)

  const hashResult = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(hashResult.hit).toBe(true)
})
