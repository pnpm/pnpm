import { expect, test } from '@jest/globals'
import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'

import { testDefaults } from '../utils/index.js'

const isOdd011ReleaseDate = new Date(2016, 11, 7 - 2) // 0.1.1 was released at 2016-12-07T07:18:01.205Z
const diff = Date.now() - isOdd011ReleaseDate.getTime()
const minimumReleaseAge = diff / (60 * 1000) // converting to minutes

// A very high value that makes ALL versions immature (cutoff date would be before any version was published)
const allImmatureMinimumReleaseAge = Date.now() / (60 * 1000)

test('minimumReleaseAge prevents installation of versions that do not meet the required publish date cutoff', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], testDefaults({ minimumReleaseAge }))

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('minimumReleaseAge is ignored for packages in the minimumReleaseAgeExclude array', async () => {
  prepareEmpty()

  const opts = testDefaults({ minimumReleaseAge, minimumReleaseAgeExclude: ['is-odd'] })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge is ignored for packages in the minimumReleaseAgeExclude array, using a pattern', async () => {
  prepareEmpty()

  const opts = testDefaults({ minimumReleaseAge, minimumReleaseAgeExclude: ['is-*'] })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge is ignored for specific exact versions in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  const opts = testDefaults({
    minimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd@0.1.2'],
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  // 0.1.2 is excluded, so it should be installed despite being newer than minimumReleaseAge
  expect(manifest.dependencies!['is-odd']).toBe('~0.1.2')
})

test('minimumReleaseAge applies to versions not in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  const opts = testDefaults({
    minimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd@0.1.0'],
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  // 0.1.2 is NOT excluded (only 0.1.0 is), so minimumReleaseAge applies
  // This should install 0.1.0 which is old enough
  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('minimumReleaseAge falls back to immature version when no mature version satisfies the range (non-strict mode)', async () => {
  prepareEmpty()

  // With non-strict mode (default), falls back to installing an immature version.
  // The fallback picks the lowest matching version (0.1.0), which differs from
  // normal resolution without minimumReleaseAge that would pick the highest (0.1.2).
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], opts)

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('minimumReleaseAge throws when no mature version satisfies the range and strict mode is enabled', async () => {
  prepareEmpty()

  await expect(async () => {
    const opts = testDefaults(
      { minimumReleaseAge: allImmatureMinimumReleaseAge },
      { strictPublishedByCheck: true }
    )
    await addDependenciesToPackage({}, ['is-odd@0.1'], opts)
  }).rejects.toThrow(/does not meet the minimumReleaseAge constraint/)
})

test('time-based resolution repopulates missing lockfile time entries on re-install', async () => {
  // Regression test: when the npm-resolver fast path (peekManifestFromStore) is
  // taken on a re-install, it must surface publishedAt from the lockfile rather
  // than returning undefined — otherwise lockfiles whose `time:` block is missing
  // entries can never recover them, which breaks downstream time-based filtering
  // for packages with version-pinned optional/platform deps.
  prepareEmpty()
  const opts = testDefaults({ minimumReleaseAge: 1, resolutionMode: 'time-based' })

  const { updatedManifest } = await addDependenciesToPackage({}, ['is-positive@1.0.0'], opts)

  const lockfileAfterFirstInstall = (await readWantedLockfile('.', { ignoreIncompatible: false }))!
  expect(Object.keys(lockfileAfterFirstInstall.time ?? {}).length).toBeGreaterThan(0)

  // Simulate a lockfile whose time entries were dropped (e.g. produced by an
  // older pnpm, or hand-edited).
  await writeWantedLockfile('.', { ...lockfileAfterFirstInstall, time: {} })

  await addDependenciesToPackage(updatedManifest, ['is-positive@1.0.0'], opts)

  const lockfileAfterReinstall = (await readWantedLockfile('.', { ignoreIncompatible: false }))!
  expect(lockfileAfterReinstall.time).toEqual(lockfileAfterFirstInstall.time)
})

test('throws error when semver range is used in minimumReleaseAgeExclude', async () => {
  prepareEmpty()

  await expect(async () => {
    const opts = testDefaults({
      minimumReleaseAge,
      minimumReleaseAgeExclude: ['is-odd@^0.1.1'],
    })
    await addDependenciesToPackage({}, ['is-odd@0.1'], opts)
  }).rejects.toThrow(/Invalid versions union/)
})
