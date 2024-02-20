import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage, install } from '@pnpm/core'
import { testDefaults } from '../utils'

test('time-based resolution mode', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/bravo', '@pnpm.e2e/romeo'], testDefaults({ resolutionMode: 'time-based' }))

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/bravo-dep@1.0.1',
    '/@pnpm.e2e/bravo@1.0.0',
    '/@pnpm.e2e/romeo-dep@1.0.0',
    '/@pnpm.e2e/romeo@1.0.0',
  ])
})

test('time-based resolution mode with a registry that supports the time field in abbreviated metadata', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/bravo', '@pnpm.e2e/romeo'], testDefaults({
    registrySupportsTimeField: true,
    resolutionMode: 'time-based',
  }))

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toStrictEqual([
    '/@pnpm.e2e/bravo-dep@1.0.1',
    '/@pnpm.e2e/bravo@1.0.0',
    '/@pnpm.e2e/romeo-dep@1.0.0',
    '/@pnpm.e2e/romeo@1.0.0',
  ])
})

test('the lowest version of a direct dependency is installed when resolution mode is time-based', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  }, testDefaults({ resolutionMode: 'time-based' }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/foo@100.0.0']).toBeTruthy()
  }

  manifest = await install(manifest, testDefaults({ resolutionMode: 'time-based', update: true }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/foo@100.1.0']).toBeTruthy()
  }
  expect(manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/foo': '^100.1.0',
  })
})

test('time-based resolution mode should not fail when publishedBy date cannot be calculated', async () => {
  prepareEmpty()
  await install({}, testDefaults({ resolutionMode: 'time-based' }))
})

test('the lowest version of a direct dependency is installed when resolution mode is lowest-direct', async () => {
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
    },
  }, testDefaults({ resolutionMode: 'lowest-direct' }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/pkg-with-1-dep@100.0.0']).toBeTruthy()
    expect(lockfile.packages['/@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0']).toBeTruthy()
  }

  manifest = await install(manifest, testDefaults({ resolutionMode: 'lowest-direct', update: true }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/pkg-with-1-dep@100.1.0']).toBeTruthy()
  }
  expect(manifest.dependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-1-dep': '^100.1.0',
  })
})
