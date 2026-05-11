import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.fs'

import {
  type PublishedAtLookup,
  revalidateLockfileAgainstMinimumReleaseAge,
} from '../../src/install/revalidateLockfileMinimumReleaseAge.js'

const NOW = new Date('2024-01-01T00:00:00Z').getTime()
const ONE_DAY_MIN = 24 * 60
const ONE_DAY_MS = ONE_DAY_MIN * 60 * 1000

function makeLockfile (packages: Record<string, { resolution: unknown, version?: string }>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    packages: packages as LockfileObject['packages'],
  } as LockfileObject
}

const tarballResolution = (integrity: string = 'sha512-deadbeef') => ({ integrity, tarball: '' })

test('passes when every lockfile entry was published before the cutoff', async () => {
  const lockfile = makeLockfile({
    'lodash@4.17.21': { resolution: tarballResolution() },
    'is-odd@0.1.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW - 30 * ONE_DAY_MS)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
})

test('throws when an entry was published after the cutoff', async () => {
  const lockfile = makeLockfile({
    'is-odd@0.1.2': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW - 60 * 1000) // 1 minute ago

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).rejects.toThrow(/minimumReleaseAge/)
})

test('error lists every violating package', async () => {
  const lockfile = makeLockfile({
    'fresh-a@1.0.0': { resolution: tarballResolution('sha512-a') },
    'fresh-b@2.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).rejects.toThrow(/fresh-a@1\.0\.0[\s\S]*fresh-b@2\.0\.0/)
})

test('excludes packages matched by name in minimumReleaseAgeExclude', async () => {
  const lockfile = makeLockfile({
    'fresh@1.0.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      minimumReleaseAgeExclude: ['fresh'],
      now: NOW,
    })
  ).resolves.toBeUndefined()
})

test('excludes packages matched by exact name@version', async () => {
  const lockfile = makeLockfile({
    'fresh@1.0.0': { resolution: tarballResolution('sha512-a') },
    'fresh@2.0.0': { resolution: tarballResolution('sha512-b') },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      minimumReleaseAgeExclude: ['fresh@1.0.0'],
      now: NOW,
    })
  ).rejects.toThrow(/fresh@2\.0\.0/)
})

test('skips entries whose version is not a valid semver (URL tarballs, file: refs, etc.)', async () => {
  const lockfile = makeLockfile({
    'tar-pkg@https://example.com/pkg.tgz': { resolution: tarballResolution() },
    'file-pkg@file:./pkg.tgz': { resolution: tarballResolution() },
  })
  let called = false
  const lookup: PublishedAtLookup = async () => {
    called = true; return new Date(NOW)
  }

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
  expect(called).toBe(false)
})

test('wraps minimumReleaseAgeExclude parse errors with the dedicated error code', async () => {
  const lockfile = makeLockfile({
    'fresh@1.0.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => new Date(NOW)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      minimumReleaseAgeExclude: ['fresh@^1.0.0'],
      now: NOW,
    })
  ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE' })
})

test('skips non-npm-registry resolutions (git, directory, etc.)', async () => {
  const lockfile = makeLockfile({
    'local-pkg@0.0.0': { resolution: { type: 'directory', directory: '/somewhere' } },
    'git-pkg@1.0.0': { resolution: { type: 'git', repo: 'x', commit: 'abc' } },
  })
  let called = false
  const lookup: PublishedAtLookup = async () => {
    called = true; return new Date(NOW)
  }

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
  expect(called).toBe(false)
})

test('skips entries when the lookup cannot determine a publish date', async () => {
  const lockfile = makeLockfile({
    'unknown@1.0.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => undefined

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
})

test('is a no-op when the lockfile has no packages', async () => {
  const lockfile = makeLockfile({})
  const lookup: PublishedAtLookup = async () => new Date(NOW)

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
})

test('propagates lookup errors so the user sees registry failures', async () => {
  const lockfile = makeLockfile({
    'broken@1.0.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => {
    throw new Error('registry unreachable')
  }

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).rejects.toThrow(/registry unreachable/)
})

test('treats an Invalid Date publish time as "unknown" (skips, does not throw)', async () => {
  const lockfile = makeLockfile({
    'malformed@1.0.0': { resolution: tarballResolution() },
  })
  const lookup: PublishedAtLookup = async () => new Date('not-a-date')

  await expect(
    revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
      minimumReleaseAge: ONE_DAY_MIN,
      now: NOW,
    })
  ).resolves.toBeUndefined()
})

test('does not re-query the same (name, version) for peer/patch suffix variants', async () => {
  const lockfile = makeLockfile({
    'react@18.0.0': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(peer-x)': { resolution: tarballResolution('sha512-a') },
    'react@18.0.0(patch_hash=abc)(peer-x)': { resolution: tarballResolution('sha512-a') },
  })
  const seen: Array<{ name: string, version: string }> = []
  const lookup: PublishedAtLookup = async (name, version) => {
    seen.push({ name, version })
    return new Date(NOW - 30 * ONE_DAY_MS)
  }

  await revalidateLockfileAgainstMinimumReleaseAge(lockfile, lookup, {
    minimumReleaseAge: ONE_DAY_MIN,
    now: NOW,
  })

  expect(seen).toEqual([{ name: 'react', version: '18.0.0' }])
})
