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
    lockfile: makeLockfile(),
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is empty', () => {
  recordLockfileVerified({
    cacheDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: [],
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('no-op when resolutionVerifiers is undefined', () => {
  recordLockfileVerified({
    cacheDir,
    lockfile: makeLockfile(),
    resolutionVerifiers: undefined,
  })
  expect(fs.existsSync(cacheDir)).toBe(false)
})

test('records nothing when the in-memory lockfile has no packages', async () => {
  await writeWantedLockfile(lockfileDir, makeLockfile())
  recordLockfileVerified({
    cacheDir,
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
  expect(record.lockfile.path).toBe('')
})

test('a file put at the lockfile path after the write does not inherit the verdict', async () => {
  const written = await writeWantedLockfile(lockfileDir, makeLockfile())
  recordLockfileVerified({
    cacheDir,
    lockfile: written,
    resolutionVerifiers: [mraVerifier(60)],
  })
  const replacement = {
    ...makeLockfile(),
    packages: {
      'is-positive@1.0.0': { resolution: { tarball: 'https://evil.example/is-positive.tgz' } },
    },
  } as unknown as LockfileObject
  await writeWantedLockfile(lockfileDir, replacement)
  const loaded = (await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }))!

  const result = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(result.hit).toBe(false)
})

test('records a cache entry that the next install hits by content, then by the stat it indexes', async () => {
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
    lockfile: written,
    resolutionVerifiers: [mraVerifier(60)],
  })
  const loaded = (await readWantedLockfile(lockfileDir, { ignoreIncompatible: false }))!

  const byContent = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => hashObject(loaded),
  })
  expect(byContent.hit).toBe(true)

  const byStat = tryLockfileVerificationCache(cacheDir, {
    lockfilePath,
    verifiers: [mraVerifier(60)],
    hashLockfile: () => {
      throw new Error('the stat shortcut must not hash the lockfile')
    },
  })
  expect(byStat.hit).toBe(true)
})

test('refuses to record a lockfile with an invalid dependency alias or package key', () => {
  fs.writeFileSync(lockfilePath, "lockfileVersion: '9.0'\n")
  const tamperedImporter = {
    ...makeLockfile(),
    importers: {
      '.': {
        specifiers: { '../../../escape': '1.0.0' },
        dependencies: { '../../../escape': '1.0.0' },
      },
    },
  } as unknown as LockfileObject
  recordLockfileVerified({
    cacheDir,
    lockfile: tamperedImporter,
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)

  const tamperedPackageKey = {
    ...makeLockfile(),
    importers: {},
    packages: {
      '../../../escape@1.0.0': {
        resolution: { integrity: 'sha512-deadbeef' },
      },
    },
  } as unknown as LockfileObject
  recordLockfileVerified({
    cacheDir,
    lockfile: tamperedPackageKey,
    resolutionVerifiers: [mraVerifier(60)],
  })
  expect(fs.existsSync(path.join(cacheDir, 'lockfile-verified.jsonl'))).toBe(false)
})
