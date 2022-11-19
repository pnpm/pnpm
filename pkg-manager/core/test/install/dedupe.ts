import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { addDistTag } from '@pnpm/registry-mock'
import { testDefaults } from '../utils'

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
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
})

test('prefer version ranges specified for top dependencies, when doing named installation', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const manifest = await install(
    {
      dependencies: {
        '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
      },
    },
    await testDefaults()
  )
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pkg-with-1-dep'], await testDefaults())

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
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
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
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
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
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
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/101.0.0'])
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
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
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
    await testDefaults(
      {
        preferredVersions: {
          '@pnpm.e2e/dep-of-pkg-with-1-dep': {
            '100.0.0': 'version',
          },
        },
      }
    )
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
})

// Covers https://github.com/pnpm/pnpm/issues/1187
test('prefer version of package that also satisfies the range of the same package higher in the dependency graph', async () => {
  const project = prepareEmpty()
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-foo-as-dep-and-subdep'],
    await testDefaults()
  )

  const lockfile = await project.readLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '/@pnpm.e2e/foo/100.0.0',
      '/@pnpm.e2e/has-foo-as-dep-and-subdep/1.0.0',
      '/@pnpm.e2e/requires-any-foo/1.0.0',
    ]
  )
})

test('dedupe subdependency when a newer version of the same package is installed', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0', '@pnpm.e2e/pkg-with-1-dep@100.0.0'], await testDefaults())

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'], await testDefaults())

  const lockfile = await project.readLockfile()
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.1.0'])
  expect(lockfile.packages).not.toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/100.0.0'])
})
