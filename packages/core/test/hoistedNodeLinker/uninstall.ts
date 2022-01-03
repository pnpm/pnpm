import {
  PackageManifestLog,
  StatsLog,
} from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  mutateModules,
} from '@pnpm/core'
import sinon from 'sinon'
import { testDefaults } from './../utils'

test('uninstall package with no dependencies', async () => {
  const project = prepareEmpty()

  let manifest = await addDependenciesToPackage(
    {},
    ['is-negative@2.1.0'],
    await testDefaults({ save: true, nodeLinker: 'hoisted' })
  )

  const reporter = sinon.spy()
  manifest = (await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ nodeLinker: 'hoisted', save: true, reporter })))[0].manifest

  expect(reporter.calledWithMatch({
    initial: {
      dependencies: {
        'is-negative': '2.1.0',
      },
    },
    level: 'debug',
    name: 'pnpm:package-manifest',
    prefix: process.cwd(),
  } as PackageManifestLog)).toBeTruthy()
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
    removed: 1,
  } as StatsLog)).toBeTruthy()
  /* This should be fixed
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'prod',
      name: 'is-negative',
      version: '2.1.0',
    },
  } as RootLog)).toBeTruthy()
  */
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {},
    },
  } as PackageManifestLog)).toBeTruthy()

  // uninstall does not remove packages from store
  // even if they become unreferenced
  await project.storeHas('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  expect(manifest.dependencies).toStrictEqual({})
})

test('uninstall package with dependencies and do not touch other deps', async () => {
  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage(
    {},
    ['is-negative@2.1.0', 'camelcase-keys@3.0.0'],
    await testDefaults({ nodeLinker: 'hoisted', save: true })
  )
  manifest = (await mutateModules([
    {
      dependencyNames: ['camelcase-keys'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ nodeLinker: 'hoisted', pruneStore: true, save: true })))[0].manifest

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHasNot('camelcase', '3.0.0')
  await project.hasNot('camelcase')

  await project.storeHasNot('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({ 'is-negative': '2.1.0' })

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies).toStrictEqual({
    'is-negative': '2.1.0',
  })
  expect(lockfile.specifiers).toStrictEqual({
    'is-negative': '2.1.0',
  })
})
