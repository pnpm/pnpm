import {
  type PackageManifestLog,
  type StatsLog,
} from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import sinon from 'sinon'
import { testDefaults } from './../utils'

test('uninstall package with no dependencies', async () => {
  const project = prepareEmpty()

  let manifest = await addDependenciesToPackage(
    {},
    ['is-negative@2.1.0'],
    testDefaults({ save: true, nodeLinker: 'hoisted' })
  )

  const reporter = sinon.spy()
  manifest = (await mutateModulesInSingleProject({
    dependencyNames: ['is-negative'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ nodeLinker: 'hoisted', save: true, reporter }))).manifest

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
  project.storeHas('is-negative', '2.1.0')

  project.hasNot('is-negative')

  expect(manifest.dependencies).toStrictEqual({})
})

test('uninstall package with dependencies and do not touch other deps', async () => {
  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage(
    {},
    ['is-negative@2.1.0', 'camelcase-keys@3.0.0'],
    testDefaults({ nodeLinker: 'hoisted', save: true })
  )
  manifest = (await mutateModulesInSingleProject({
    dependencyNames: ['camelcase-keys'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ nodeLinker: 'hoisted', pruneStore: true, save: true }))).manifest

  project.storeHasNot('camelcase-keys', '3.0.0')
  project.hasNot('camelcase-keys')

  project.storeHasNot('camelcase', '3.0.0')
  project.hasNot('camelcase')

  project.storeHasNot('map-obj', '1.0.1')
  project.hasNot('map-obj')

  project.storeHas('is-negative', '2.1.0')
  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({ 'is-negative': '2.1.0' })

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies).toStrictEqual({
    'is-negative': {
      specifier: '2.1.0',
      version: '2.1.0',
    },
  })
})
