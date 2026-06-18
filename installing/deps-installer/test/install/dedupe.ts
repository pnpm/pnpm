import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import { writeYamlFileSync } from 'write-yaml-file'

import { testDefaults } from '../utils/index.js'

test('prefer version ranges specified for top dependencies', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/pkg-with-1-dep': '*',
      },
    },
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('does not delegate lockfile check mode to pacquet', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, testDefaults())

  const lockfileCheck = jest.fn()
  const runPacquet = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, testDefaults({
    dedupe: true,
    lockfileCheck,
    runPacquet: {
      supportsResolution: true,
      run: runPacquet,
    },
  }))

  expect(lockfileCheck).toHaveBeenCalled()
  expect(runPacquet).not.toHaveBeenCalled()
})

test('does not delegate no-lockfile installs to pacquet', async () => {
  const project = prepareEmpty()
  const runPacquet = jest.fn<() => Promise<void>>().mockResolvedValue(undefined)

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    runPacquet: {
      supportsResolution: true,
      run: runPacquet,
    },
    useLockfile: false,
  }))

  expect(runPacquet).not.toHaveBeenCalled()
  expect(project.readLockfile()).toBeFalsy()
})

test('uses the lockfile written by pacquet for post-install checks', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    allowBuilds: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': true,
    },
  }))

  const depPath = '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'
  const runPacquet = jest.fn<(opts?: { resolve?: boolean }) => Promise<void>>().mockImplementation(async () => {
    writeYamlFileSync(WANTED_LOCKFILE, {
      importers: {
        '.': {
          dependencies: {
            '@pnpm.e2e/pre-and-postinstall-scripts-example': {
              specifier: '1.0.0',
              version: '1.0.0',
            },
          },
        },
      },
      lockfileVersion: LOCKFILE_VERSION,
      packages: {
        [depPath]: {
          resolution: {
            integrity: 'sha512-test',
          },
        },
      },
      snapshots: {
        [depPath]: {},
      },
    }, { lineWidth: 1000 })
  })

  const { ignoredBuilds } = await install({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }, testDefaults({
    allowBuilds: {},
    runPacquet: {
      supportsResolution: true,
      run: runPacquet,
    },
  }))

  expect(runPacquet).toHaveBeenCalledWith({ resolve: true })
  expect(Array.from(ignoredBuilds ?? [])).toContain(depPath)
})

test('prefer version ranges specified for top dependencies, when doing named installation', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const { updatedManifest: manifest } = await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
    },
    testDefaults()
  )
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults())

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if they are aliased', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        foo: 'npm:@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
        '@pnpm.e2e/pkg-with-1-dep': '*',
      },
    },
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('prefer version ranges specified for top dependencies, even if the subdependencies are aliased', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/pkg-with-1-aliased-dep': '100.0.0',
      },
    },
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('ignore version of root dependency when it is incompatible with the indirect dependency\'s range', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      },
    },
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'])
})

test('refreshes a stale transitive pin to a higher direct-dependency version at resolution time', async () => {
  // The stale transitive pin is refreshed during resolution, so the older
  // version is never resolved or fetched (no post-resolution pruning).
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/pkg-with-1-dep@100.0.0'],
    testDefaults()
  )

  expect(project.readLockfile().packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'],
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
})

test('does not refresh an aliased transitive dependency', async () => {
  // pkg-with-1-aliased-dep depends on `dep: npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0`.
  // An `npm:` specifier is not a plain semver range, so the refresh skips
  // the edge and the older version is kept (no misfire on aliases).
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0'],
    testDefaults()
  )

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'],
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
})

test('refreshing a stale transitive pin is idempotent', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/pkg-with-1-dep@100.0.0'],
    testDefaults()
  )

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const { updatedManifest: manifestWithBoth } = await addDependenciesToPackage(
    manifest,
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'],
    testDefaults()
  )

  const convergedPackages = project.readLockfile().packages
  expect(convergedPackages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])

  // A second install over the converged lockfile must not reintroduce or
  // churn the refreshed edge.
  await install(manifestWithBoth, testDefaults())
  expect(project.readLockfile().packages).toStrictEqual(convergedPackages)
})

test('prefer dist-tag specified for top dependency', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'stable' })

  await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': 'stable',
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      },
    },
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

test('prefer version ranges passed in via opts.preferredVersions', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const project = prepareEmpty()

  await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
        '@pnpm.e2e/pkg-with-1-dep': '*',
      },
    },
    testDefaults(
      {
        preferredVersions: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': {
            '100.0.0': 'version',
          },
        },
      }
    )
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/1187
test('prefer version of package that also satisfies the range of the same package higher in the dependency graph', async () => {
  const project = prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-foo-as-dep-and-subdep'],
    testDefaults()
  )

  const lockfile = project.readLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '@pnpm.e2e/foo@100.0.0',
      '@pnpm.e2e/has-foo-as-dep-and-subdep@1.0.0',
      '@pnpm.e2e/requires-any-foo@1.0.0',
    ]
  )
})

test('when resolving dependencies, prefer versions that are used by direct dependencies over versions used in subdeps', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await install({
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
      '@pnpm.e2e/has-foo-100.1.0-dep-1': '1.0.0',
      '@pnpm.e2e/has-foo-100.1.0-dep-2': '1.0.0',
    },
  }, testDefaults())

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/has-foo-100.0.0-range-dep'], testDefaults())

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/has-foo-100.0.0-range-dep@1.0.0']).toHaveProperty(['dependencies', '@pnpm.e2e/foo'], '100.0.0')
})

test('when resolving dependencies, prefer versions that are used by direct dependencies over versions used in subdeps #2', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await install({
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
      '@pnpm.e2e/has-foo-100.1.0-dep-1': '1.0.0',
      '@pnpm.e2e/has-foo-100.1.0-dep-2': '1.0.0',
    },
  }, testDefaults())

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/has-foo-100.0.0-range-dep'], testDefaults())

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/has-foo-100.0.0-range-dep@1.0.0']).toHaveProperty(['dependencies', '@pnpm.e2e/foo'], '100.0.0')
})
