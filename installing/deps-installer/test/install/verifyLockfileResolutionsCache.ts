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

// Helpers — most tests use a stand-in for the npm minimumReleaseAge
// verifier. The cache layer is policy-neutral, so this could be any
// verifier shape.
function mraVerifier (current: number): VerifierCacheIdentity {
  return {
    policy: { minimumReleaseAge: current },
    canTrustPastCheck: (cached) => {
      const past = cached.minimumReleaseAge
      return typeof past === 'number' && past >= current
    },
  }
}

describe('tryLockfileVerificationCache', () => {
  test('miss when the cache file does not exist', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('miss when the lockfile path is not in the cache', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    // Seed an unrelated record.
    recordVerification(cacheDir, {
      lockfilePath: path.join(tmpDir, 'other-lockfile.yaml'),
      verifiers: [mraVerifier(60)],
    })
    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('stat-only hit when size, mtime, and inode all match', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when the file size differs from the cached record', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Append bytes — size changes, so the fast path bails immediately.
    await fs.promises.appendFile(lockfilePath, 'extra: bytes\n')

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('hash-fallback hit when size matches but mtime/inode were reset', async () => {
    // Simulate a CI checkout: same content, fresh inode + mtime.
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Write to a different path, unlink the original, rename in. This
    // guarantees a different inode while keeping the same content.
    const sibling = path.join(tmpDir, 'pnpm-lock-2.yaml')
    await fs.promises.writeFile(sibling, 'lockfileVersion: \'9.0\'\n')
    await fs.promises.rm(lockfilePath)
    await fs.promises.rename(sibling, lockfilePath)

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when content changed even if size happens to match', async () => {
    await fs.promises.writeFile(lockfilePath, 'aaaaaaaaaaaa')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Same byte length, different content — hash check is what rejects.
    await fs.promises.rm(lockfilePath)
    await fs.promises.writeFile(lockfilePath, 'bbbbbbbbbbbb')

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('miss when a verifier rejects the cached policy', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    // Today's policy is stricter than the cached one.
    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(120)],
    })
    expect(result.hit).toBe(false)
  })

  test('hit when a verifier accepts the cached policy', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(1440)] })

    // Today's policy is weaker — the stricter cached run still covers it.
    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })

  test('miss when the cached policy lacks a field the current verifier reads', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    // Seed a record whose policy doesn't have minimumReleaseAge.
    recordVerification(cacheDir, {
      lockfilePath,
      verifiers: [{
        policy: { someOther: 'value' },
        canTrustPastCheck: () => true,
      }],
    })

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('hit when every verifier trusts its share of the merged cached policy', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const verifiers: VerifierCacheIdentity[] = [
      mraVerifier(60),
      {
        policy: { trustedPublishers: ['foo-org'] },
        canTrustPastCheck: (cached) => Array.isArray(cached.trustedPublishers) &&
          cached.trustedPublishers.includes('foo-org'),
      },
    ]
    recordVerification(cacheDir, { lockfilePath, verifiers })

    const result = tryLockfileVerificationCache(cacheDir, { lockfilePath, verifiers })
    expect(result.hit).toBe(true)
  })

  test('miss when the lockfile no longer exists', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    await fs.promises.rm(lockfilePath)

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(false)
  })

  test('latest record per path wins when the cache has multiple appends', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')

    // Earlier record under a stricter cutoff.
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    // Later record under a weaker cutoff that does satisfy 120.
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(120)] })

    const result = tryLockfileVerificationCache(cacheDir, {
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

    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const result = tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60)],
    })
    expect(result.hit).toBe(true)
  })
})

describe('recordVerification', () => {
  test('writes a JSONL record with a merged policy bag', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(1)
    const record = JSON.parse(lines[0]) as Record<string, unknown> & {
      lockfile: Record<string, unknown>
    }
    expect(record).toMatchObject({
      lockfile: { path: lockfilePath },
      policy: { minimumReleaseAge: 60 },
    })
    expect(typeof record.lockfile.hash).toBe('string')
    expect(typeof record.verifiedAt).toBe('string')
    expect(typeof record.lockfile.size).toBe('number')
    expect(typeof record.lockfile.mtimeNs).toBe('string')
    expect(typeof record.lockfile.inode).toBe('number')
  })

  test('merges policy fields across verifiers into a single bag', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, {
      lockfilePath,
      verifiers: [
        mraVerifier(60),
        {
          policy: { trustedPublishers: ['foo-org', 'pnpm'] },
          canTrustPastCheck: () => true,
        },
      ],
    })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const record = JSON.parse(raw.trim()) as { policy: Record<string, unknown> }
    expect(record.policy).toEqual({
      minimumReleaseAge: 60,
      trustedPublishers: ['foo-org', 'pnpm'],
    })
  })

  test('shared policy field is stored once, not duplicated', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    // Two verifiers both contribute minimumReleaseAge with the same value
    // — the merged bag stores it once.
    recordVerification(cacheDir, {
      lockfilePath,
      verifiers: [mraVerifier(60), mraVerifier(60)],
    })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const record = JSON.parse(raw.trim()) as { policy: Record<string, unknown> }
    expect(record.policy).toEqual({ minimumReleaseAge: 60 })
  })

  test('silently skips when the lockfile is missing', async () => {
    expect(
      recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })
    ).toBeUndefined()
    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    await expect(fs.promises.access(cacheFile)).rejects.toThrow()
  })

  test('appends without rewriting previous lines', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, { lockfilePath, verifiers: [mraVerifier(60)] })

    const otherLockfile = path.join(tmpDir, 'other-lockfile.yaml')
    await fs.promises.writeFile(otherLockfile, 'lockfileVersion: \'9.0\'\n')
    recordVerification(cacheDir, {
      lockfilePath: otherLockfile,
      verifiers: [mraVerifier(60)],
    })

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(2)
    const paths = lines.map((line) => (JSON.parse(line) as { lockfile: { path: string } }).lockfile.path)
    expect(paths).toEqual(expect.arrayContaining([lockfilePath, otherLockfile]))
  })
})
