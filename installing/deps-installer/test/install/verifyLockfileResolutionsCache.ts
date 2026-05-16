import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import {
  recordVerification,
  tryLockfileVerificationCache,
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

describe('tryLockfileVerificationCache', () => {
  test('miss when the cache file does not exist', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(false)
  })

  test('miss when the lockfile path is not in the cache', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    // Seed an unrelated record.
    await recordVerification(cacheDir, {
      lockfilePath: path.join(tmpDir, 'other-lockfile.yaml'),
      minimumReleaseAge: 60,
    })
    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(false)
  })

  test('stat-only hit when size, mtime, and inode all match', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(true)
  })

  test('miss when the file size differs from the cached record', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    // Append bytes — size changes, so the fast path bails immediately.
    await fs.promises.appendFile(lockfilePath, 'extra: bytes\n')

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(false)
  })

  test('hash-fallback hit when size matches but mtime/inode were reset', async () => {
    // Simulate a CI checkout: same content, fresh inode + mtime. Easiest to
    // reproduce on a real fs by writing the file twice — node will allocate a
    // new inode the second time, and the mtime will move forward.
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    // Use `cp` semantics: write to a different path, unlink the original,
    // rename in. This guarantees a different inode while keeping the same
    // content so the size still matches but stat-only comparison fails.
    const sibling = path.join(tmpDir, 'pnpm-lock-2.yaml')
    await fs.promises.writeFile(sibling, 'lockfileVersion: \'9.0\'\n')
    await fs.promises.rm(lockfilePath)
    await fs.promises.rename(sibling, lockfilePath)

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(true)
  })

  test('miss when content changed even if size happens to match', async () => {
    await fs.promises.writeFile(lockfilePath, 'aaaaaaaaaaaa')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    // Same byte length, different content. Stat-only might still mismatch on
    // mtime; the hash check is what rejects this case.
    await fs.promises.rm(lockfilePath)
    await fs.promises.writeFile(lockfilePath, 'bbbbbbbbbbbb')

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(false)
  })

  test('miss when current minimumReleaseAge is stricter than the cached value', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 120,
    })
    expect(result.hit).toBe(false)
  })

  test('hit when current minimumReleaseAge is weaker than the cached value', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 1440 })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(true)
  })

  test('miss when the lockfile no longer exists', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })
    await fs.promises.rm(lockfilePath)

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(false)
  })

  test('latest record per path wins when the cache has multiple appends', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')

    // Earlier record under a stricter cutoff that wouldn't satisfy 120.
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })
    // Later record under a weaker cutoff that does satisfy 120.
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 120 })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 120,
    })
    expect(result.hit).toBe(true)
  })

  test('malformed lines are ignored, not propagated', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await fs.promises.mkdir(cacheDir, { recursive: true })
    const cacheFile = path.join(cacheDir, 'minimum-release-age-verified.jsonl')
    await fs.promises.writeFile(cacheFile, '{not json\n\n')

    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    const result = await tryLockfileVerificationCache(cacheDir, {
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(result.hit).toBe(true)
  })
})

describe('recordVerification', () => {
  test('writes a JSONL record under <cacheDir>/minimum-release-age-verified.jsonl', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    const cacheFile = path.join(cacheDir, 'minimum-release-age-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(1)
    const record = JSON.parse(lines[0]) as Record<string, unknown>
    expect(record).toMatchObject({
      lockfilePath,
      minimumReleaseAge: 60,
    })
    expect(typeof record.lockfileHash).toBe('string')
    expect(typeof record.verifiedAt).toBe('string')
    expect(typeof record.lockfileFileSize).toBe('number')
    expect(typeof record.lockfileMtimeNs).toBe('string')
    expect(typeof record.lockfileInode).toBe('number')
  })

  test('silently skips when the lockfile is missing', async () => {
    await expect(
      recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })
    ).resolves.toBeUndefined()
    // No record means no file — the call is a noop.
    const cacheFile = path.join(cacheDir, 'minimum-release-age-verified.jsonl')
    await expect(fs.promises.access(cacheFile)).rejects.toThrow()
  })

  test('appends without rewriting previous lines', async () => {
    await fs.promises.writeFile(lockfilePath, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath, minimumReleaseAge: 60 })

    const otherLockfile = path.join(tmpDir, 'other-lockfile.yaml')
    await fs.promises.writeFile(otherLockfile, 'lockfileVersion: \'9.0\'\n')
    await recordVerification(cacheDir, { lockfilePath: otherLockfile, minimumReleaseAge: 60 })

    const cacheFile = path.join(cacheDir, 'minimum-release-age-verified.jsonl')
    const raw = await fs.promises.readFile(cacheFile, 'utf8')
    const lines = raw.split('\n').filter(Boolean)
    expect(lines).toHaveLength(2)
    const paths = lines.map((line) => (JSON.parse(line) as { lockfilePath: string }).lockfilePath)
    expect(paths).toEqual(expect.arrayContaining([lockfilePath, otherLockfile]))
  })
})
