import { expect, jest, test } from '@jest/globals'
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
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    // Acknowledge the policy violations without aborting — this test
    // only inspects the resolved manifest. resolveDependencies refuses
    // to proceed if violations fire and no handler is wired.
    handleResolutionPolicyViolations: async () => {},
  })

  expect(manifest.dependencies!['is-odd']).toBe('~0.1.0')
})

test('strict minimumReleaseAge surfaces every immature pick via handleResolutionPolicyViolations, then aborts', async () => {
  // Pre-refactor strict mode threw at the resolver on the first immature
  // pick (forcing a discover-by-loop dance, #10488). With always-defer the
  // resolver records every immature pick inline; the install command (here
  // simulated via the hook) decides what to do once it has the full set.
  prepareEmpty()
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  const seen: string[] = []
  await expect(addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    handleResolutionPolicyViolations: async (violations) => {
      for (const v of violations) seen.push(`${v.name}@${v.version}`)
      throw new Error('immature picks rejected')
    },
  })).rejects.toThrow(/immature picks rejected/)
  expect(seen).toContain('is-odd@0.1.0')
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

test('loose mode surfaces immature fresh picks in the install result', async () => {
  prepareEmpty()

  // Every version is younger than the cutoff. With strict mode off the
  // resolver's lowest-version fallback installs the immature version,
  // and the post-resolution scan in `mutateModules` reports it back via
  // `resolutionPolicyViolations`. The CLI command filters by code to
  // persist the entries to `minimumReleaseAgeExclude`.
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  const result = await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    // Acknowledge the violations without aborting so the install
    // proceeds and the result can be inspected.
    handleResolutionPolicyViolations: async () => {},
  })

  expect(result.resolutionPolicyViolations).toContainEqual(
    expect.objectContaining({
      name: 'is-odd',
      version: '0.1.0',
      code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    })
  )
})

test('pacquet materializes after pnpm resolves when policy violations must be surfaced', async () => {
  prepareEmpty()

  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  const runPacquet = jest.fn<(opts?: { filterResolvedProgress?: boolean, resolve?: boolean }) => Promise<void>>().mockResolvedValue(undefined)
  const result = await install({
    dependencies: {
      'is-odd': '0.1',
    },
  }, {
    ...opts,
    handleResolutionPolicyViolations: async () => {},
    runPacquet: {
      supportsResolution: true,
      run: runPacquet,
    },
  })

  expect(runPacquet).toHaveBeenCalledWith({ filterResolvedProgress: true })
  expect(runPacquet).not.toHaveBeenCalledWith({ resolve: true })
  expect(result.resolutionPolicyViolations).toContainEqual(
    expect.objectContaining({
      name: 'is-odd',
      version: '0.1.0',
      code: 'MINIMUM_RELEASE_AGE_VIOLATION',
    })
  )
})

test('versions excluded via minimumReleaseAgeExclude are not surfaced as violations', async () => {
  prepareEmpty()

  const opts = testDefaults({
    minimumReleaseAge: allImmatureMinimumReleaseAge,
    minimumReleaseAgeExclude: ['is-odd'],
  })
  // is-odd is excluded, but `is-odd@0.1.2` pulls in is-buffer / is-number /
  // kind-of transitively — those still produce policy violations. Wire a
  // no-op handler to acknowledge them.
  const result = await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    handleResolutionPolicyViolations: async () => {},
  })

  // is-odd is excluded by policy — the install installed 0.1.2 (the highest in
  // range) treating it as fully trusted. The verifier short-circuits on the
  // excluded entry, so it doesn't end up in the violations array — otherwise
  // every install would re-add the same exclude entry the user just dismissed.
  expect(result.resolutionPolicyViolations.find((v) => v.name === 'is-odd')).toBeUndefined()
})

test('handleResolutionPolicyViolations throwing aborts the install before the lockfile is written', async () => {
  // Simulates the strict-mode interactive prompt rejecting the immature
  // picks. The hook runs after the new lockfile is built but before it's
  // written to disk; throwing unwinds the install in its pre-install state.
  prepareEmpty()
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  await expect(addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    handleResolutionPolicyViolations: async () => {
      throw new Error('user denied')
    },
  })).rejects.toThrow(/user denied/)

  // The lockfile must NOT have been written — the throw fires before the
  // resolver finishes, so no on-disk side effects.
  await expect(readWantedLockfile('.', { ignoreIncompatible: false })).resolves.toBeNull()
})

test('resolveDependencies throws if violations fire but no handleResolutionPolicyViolations is wired', async () => {
  // Safety net: the policy contract is "every pick that trips a check
  // produces a violation that gets handled". A caller that opted into a
  // policy but forgot to wire the handler would otherwise silently drop
  // the violations and land policy-rejected versions in the lockfile.
  prepareEmpty()
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  await expect(addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    // Explicitly omit handleResolutionPolicyViolations.
    handleResolutionPolicyViolations: undefined,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_RESOLUTION_POLICY_VIOLATIONS_UNHANDLED' })
})

test('handleResolutionPolicyViolations approval lets the install proceed cleanly', async () => {
  prepareEmpty()
  const opts = testDefaults({ minimumReleaseAge: allImmatureMinimumReleaseAge })
  const result = await addDependenciesToPackage({}, ['is-odd@0.1'], {
    ...opts,
    handleResolutionPolicyViolations: async (violations) => {
      // The real install command would inspect the violations and run
      // an enquirer prompt here. The test just confirms the hook gets a
      // full set and returns to approve.
      expect(violations.some((v) => v.name === 'is-odd' && v.version === '0.1.0')).toBe(true)
    },
  })

  expect(result.updatedManifest.dependencies!['is-odd']).toBe('~0.1.0')
})
