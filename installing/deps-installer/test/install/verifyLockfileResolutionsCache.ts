import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import {
  recordVerification,
  tryLockfileVerificationCache,
  type VerifierCacheIdentity,
} from '../../src/install/verifyLockfileResolutionsCache.js'

let tmpDir!: string
let cacheDir!: string
let lockfilePath!: string

beforeEach(async () => {
  tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-verify-cache-'))
  cacheDir = path.join(tmpDir, 'cache')
  lockfilePath = path.join(tmpDir, 'pnpm-lock.yaml')
})

afterEach(async () => {
  await fs.promises.rm(tmpDir, { recursive: true, force: true })
})

// Helpers — most tests use the npm.minimumReleaseAge verifier as a
// concrete stand-in. The cache layer is policy-neutral, so this could be
// any verifier shape.
function mraVerifier (current: number): VerifierCacheIdentity {
  return {
    key: 'npm.minimumReleaseAge',
    policy: current,
    satisfies: (cached) => typeof cached === 'number' && cached >= current,
  }
}

describe('tryLockfileVerificationCache', () => {
  test('miss when the cache file does not exist', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('miss when the lockfile path is not in the cache', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    // Seed an unrelated record.
    await recordVerification(cacheDir, {
      lockfilePath: path.join(tmpDir, 'other-lockfile.yaml'),
      verifiers: [mraVerifier(60)],
    })
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('stat-only hit when size, mtime, and inode all match', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when the file size differs from the cached record', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Append bytes — size changes, so the fast path bails immediately.
    await fs.promises.appendFile(lockfilePath, 'extra: bytes\n')

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('hash-fallback hit when size matches but mtime/inode were reset', async () => {
    // Simulate a CI checkout: same content, fresh inode + mtime.
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Write to a different path, unlink the original, rename in. This
    // guarantees a different inode while keeping the same content.
    const sibling = path.join(tmpDir, 'pnpm-lock-2.yaml')
    await fs.promises.writeFile(sibling, 'lockfileVersion: \'9.0\'\n')
    await fs.promises.rm(lockfilePath)
    await fs.promises.rename(sibling, lockfilePath)

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when content changed even if size happens to match', async () => {
    await fs.promises.writeFile(lockfilePath, 'aaaaaaaaaaaa')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Same byte length, different content — hash check is what rejects.
    await fs.promises.rm(lockfilePath)
    await fs.promises.writeFile(lockfilePath, 'bbbbbbbbbbbb')

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('miss when a verifier rejects the cached policy', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Today's policy is stricter than the cached one.
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(120)],
    })
    expect(result.hit).toBe(false)
  })

  test('hit when a verifier accepts the cached policy', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(1440)] })

    // Today's policy is weaker — the stricter cached run still covers it.
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when an active verifier has no slot in the cached record', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // A new verifier has joined since the record was written. The cache
    // can't tell us anything about it, so we must rerun the gate.
    const newVerifier: VerifierCacheIdentity = {
      key: 'jsr.trustedPublishers',
      policy: ['foo-org'],
      satisfies: () => true,
    }
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60), newVerifier],
    })
    expect(result.hit).toBe(false)
  })

  test('hit when all active verifiers are satisfied', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const verifiers: VerifierCacheIdentity[] = [
      mraVerifier(60),
      {
        key: 'example.fixed',
        policy: 'x',
        satisfies: (cached) => cached === 'x',
      },
    ]
    await recordVerification(cacheDir, { lockfilePath, verifiers })

    const result = await tryLockfileVerificationCache(cacheDir, { lockfilePath, verifiers })
    expect(result.hit).toBe(true)
  })

  test('miss when the lockfile no longer exists', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    await fs.promises.rm(lockfilePath)

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('latest record per path wins when the cache has multiple appends', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')

    // Earlier record under a stricter cutoff.
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    // Later record under a weaker cutoff that does satisfy 120.
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(120)] })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(120)],
    })
    expect(result.hit).toBe(true)
  })

  test('malformed lines are ignored, not propagated', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await fs.promises.mkdir(cacheDir, { recursive: true })
    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    await fs.promises.writeFile(cacheFile, '{not json\n\n')

    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })
})

describe('recordVerification', () => {
  test('writes a JSONL record under <cacheDir>/lockfile-verified.jsonl', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(1)
    const record = JSON.parse(lines[0]) as Record<string, unknown>
    expect(record).toMatchObject({
      lockfilePath,
      verifiers: { 'npm.minimumReleaseAge': 60 },
    })
    expect(typeof record.lockfileHash).toBe('string')
    expect(typeof record.verifiedAt).toBe('string')
    expect(typeof record.lockfileFileSize).toBe('number')
    expect(typeof record.lockfileMtimeNs).toBe('string')
    expect(typeof record.lockfileInode).toBe('number')
  })

  test('records every active verifier slot, keyed by verifier id', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, {
      lockfilePath,
      verifiers: [
        mraVerifier(60),
        {
          key: 'jsr.trustedPublishers',
          policy: ['foo-org', 'pnpm'],
          satisfies: () => true,
        },
      ],
    })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const record = JSON.parse(raw.trim()) as { verifiers: Record<string, unknown> }
    expect(record.verifiers).toEqual({
      'npm.minimumReleaseAge': 60,
      'jsr.trustedPublishers': ['foo-org', 'pnpm'],
    })
  })

  test('silently skips when the lockfile is missing', async () => {
    await expect(
      recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    ).resolves.toBeUndefined()
    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    await expect(fs.promises.access(cacheFile)).rejects.toThrow()
  })

  test('appends without rewriting previous lines', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const otherLockfile = path.join(tmpDir, 'other-lockfile.yaml')
    await fs.promises.writeFile(otherLockfile, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, {
      lockfilePath: otherLockfile,
      verifiers: [mraVerifier(60)],
    })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(2)
    const paths = lines.map((line) => (JSON.parse(line) as { lockfilePath: string }).lockfilePath)
    expect(paths).toEqual(expect.arrayContaining([lockfilePath, otherLockfile]))
  })
})
