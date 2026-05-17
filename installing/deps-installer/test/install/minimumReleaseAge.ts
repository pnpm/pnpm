import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
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

test('minimumReleaseAge is enforced on an existing lockfile entry that does not meet the cutoff', async () => {
  prepareEmpty()

  // Generate a lockfile without minimumReleaseAge — picks the latest 0.1.x (= 0.1.2),
  // which is immature relative to isOdd011ReleaseDate.
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1.2'], testDefaults())
  expect(manifest.dependencies!['is-odd']).toBe('0.1.2')

  // Subsequent install enables minimumReleaseAge in strict mode. The lockfile
  // already has 0.1.2 so resolution is normally skipped; the revalidation pass
  // must catch this. `minimumReleaseAgeStrict` mirrors the CLI config reader's
  // auto-true behavior when the user explicitly sets `minimumReleaseAge`.
  await expect(
    install(manifest, testDefaults({ minimumReleaseAge, minimumReleaseAgeStrict: true }))
  ).rejects.toThrow(/minimumReleaseAge/)
})

test('minimumReleaseAge revalidation respects minimumReleaseAgeExclude on an existing lockfile entry', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1.2'], testDefaults())
  expect(manifest.dependencies!['is-odd']).toBe('0.1.2')

  // is-odd@0.1.2 brings in is-buffer and kind-of as transitive deps; both were
  // published after the cutoff in this test, so all three must be excluded for
  // the install to succeed.
  await expect(
    install(manifest, testDefaults({
      minimumReleaseAge,
      minimumReleaseAgeStrict: true,
      minimumReleaseAgeExclude: ['is-odd@0.1.2', 'is-buffer', 'kind-of'],
    }))
  ).resolves.toBeDefined()
})

test('minimumReleaseAge is enforced on pre-existing lockfile entries during pnpm add', async () => {
  prepareEmpty()

  // Populate the lockfile with an immature entry without the policy.
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1.2'], testDefaults())
  expect(manifest.dependencies!['is-odd']).toBe('0.1.2')

  // Subsequent `pnpm add` for an unrelated package would normally let
  // is-odd@0.1.2 survive resolution as-is via the resolver's
  // peekManifestFromStore fast path, bypassing the policy. The post-resolution
  // gate must catch it.
  await expect(
    addDependenciesToPackage(
      manifest,
      ['is-positive@1.0.0'],
      testDefaults({ minimumReleaseAge, minimumReleaseAgeStrict: true })
    )
  ).rejects.toThrow(/minimumReleaseAge/)
})

test('the lockfile minimumReleaseAge gate runs in loose mode too', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1.2'], testDefaults())
  expect(manifest.dependencies!['is-odd']).toBe('0.1.2')

  // Loose mode no longer skips the verifier — once auto-collect makes every
  // accepted-immature pin explicit in `minimumReleaseAgeExclude`, running
  // the verifier in loose mode is what keeps the manifest in sync with the
  // lockfile. A pre-existing immature lockfile entry that isn't yet on the
  // exclude list is rejected here, same as strict mode.
  await expect(
    install(manifest, testDefaults({ minimumReleaseAge }))
  ).rejects.toThrow(/minimumReleaseAge/)
})

test('the lockfile minimumReleaseAge gate accepts loose-mode entries already on the exclude list', async () => {
  prepareEmpty()

  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1.2'], testDefaults())

  // is-odd@0.1.2 pulls in is-buffer and kind-of transitively. With the exclude
  // list pre-populated (as the auto-collect would have produced on a previous
  // install), the loose-mode verifier accepts all three and the install
  // completes — the steady-state shape this feature is built around.
  await expect(
    install(manifest, testDefaults({
      minimumReleaseAge,
      minimumReleaseAgeStrict: false,
      minimumReleaseAgeExclude: ['is-odd@0.1.2', 'is-buffer', 'kind-of'],
    }))
  ).resolves.toBeDefined()
})

test('loose mode reports immature fresh picks to onImmaturePick', async () => {
  prepareEmpty()

  // Every version is younger than the cutoff. With strict mode off the
  // resolver's lowest-version fallback installs the immature version and
  // notifies the install layer, which the CLI command wires to the
  // workspace manifest writer.
  const picks: Array<{ name: string, version: string }> = []
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    onImmaturePick: (pkg) => { picks.push(pkg) },
  })

  expect(picks).toContainEqual({ name: 'is-odd', version: '0.1.0' })
})

test('strict mode does not invoke onImmaturePick (resolver throws first)', async () => {
  prepareEmpty()

  const picks: Array<{ name: string, version: string }> = []
  const opts = testDefaults(
    { minimumReleaseAge: allImmatureMinimumReleaseAge },
    { strictPublishedByCheck: true }
  )
  await expect(addDependenciesToPackage(
    {},
    ['is-odd@0.1'],
    { ...opts, onImmaturePick: (pkg) => { picks.push(pkg) } }
  )).rejects.toThrow(/does not meet the minimumReleaseAge constraint/)

  // Strict mode short-circuits in `pickRespectingMinReleaseAge` before the
  // fallback that triggers the notification. Verifying the counter stays
  // at 0 here means callers can safely wire `onImmaturePick` unconditionally
  // and rely on the resolver to gate.
  expect(picks).toEqual([])
})

test('versions excluded via minimumReleaseAgeExclude are not reported', async () => {
  prepareEmpty()

  const picks: Array<{ name: string, version: string }> = []
  const opts = testDefaults({
    minimumReleaseAge: allImmatureMinimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd'],
  })
  await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    onImmaturePick: (pkg) => { picks.push(pkg) },
  })

  // is-odd is excluded by policy — the install installed 0.1.2 (the highest in
  // range) treating it as fully trusted. Re-recording it as immature would
  // create a stuttering loop where every install re-adds the same exclude
  // entry the user just dismissed.
  expect(picks.find((p) => p.name === 'is-odd')).toBeUndefined()
})

test('deferImmatureDecision lets strict mode collect every immature pick instead of throwing on the first', async () => {
  // Strict mode normally throws `NO_MATURE_MATCHING_VERSION` on the first
  // immature transitive, forcing the discover-by-loop dance (#10488). With
  // `deferImmatureDecision: true` the resolver falls back to the lowest
  // matching version like loose mode does, and `onImmaturePick` fires once
  // per immature pick — so the install command can surface the full set to
  // the user in one prompt.
  prepareEmpty()
  const picks: Array<{ name: string, version: string }> = []
  const opts = testDefaults(
    { minimumReleaseAge: allImmatureMinimumReleaseAge },
    { strictPublishedByCheck: true }
  )
  await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    deferImmatureDecision: true,
    onImmaturePick: (pkg) => { picks.push(pkg) },
  })

  expect(picks).toContainEqual({ name: 'is-odd', version: '0.1.0' })
})

test('confirmImmaturePicks aborts the install before the lockfile is written', async () => {
  // Simulates the strict-mode interactive prompt rejecting the immature
  // picks. The callback throws between main resolution and peer-dep
  // resolution; `resolveDependencies` unwinds, which leaves the install in
  // its pre-install state — no lockfile written, no package.json changes,
  // no node_modules linking.
  prepareEmpty()
  const opts = testDefaults(
    { minimumReleaseAge: allImmatureMinimumReleaseAge },
    { strictPublishedByCheck: true }
  )
  await expect(addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    deferImmatureDecision: true,
    confirmImmaturePicks: async () => {
      throw new Error('user denied')
    },
  })).rejects.toThrow(/user denied/)

  // The lockfile must NOT have been written — the throw fires before the
  // resolver finishes, so no on-disk side effects.
  await expect(readWantedLockfile('.', { ignoreIncompatible: false })).resolves.toBeNull()
})

test('confirmImmaturePicks approval lets the install proceed cleanly', async () => {
  prepareEmpty()
  const picks: Array<{ name: string, version: string }> = []
  const opts = testDefaults(
    { minimumReleaseAge: allImmatureMinimumReleaseAge },
    { strictPublishedByCheck: true }
  )
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    deferImmatureDecision: true,
    onImmaturePick: (pkg) => { picks.push(pkg) },
    confirmImmaturePicks: async () => {
      // The real install command would surface the gathered set via
      // enquirer here; in this test we simulate the user clicking
      // "approve" by simply returning.
    },
  })

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
  expect(picks).toContainEqual({ name: 'is-odd', version: '0.1.0' })
})
