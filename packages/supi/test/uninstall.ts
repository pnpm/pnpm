import { promisify } from 'util'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  PackageManifestLog,
  RootLog,
  StatsLog,
} from '@pnpm/core-loggers'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { pathToLocalPkg } from '@pnpm/test-fixtures'
import { PackageManifest } from '@pnpm/types'
import readYamlFile from 'read-yaml-file'
import {
  addDependenciesToPackage,
  link,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from './utils'
import path = require('path')
import existsSymlink = require('exists-link')
import ncpCB = require('ncp')
import exists = require('path-exists')
import sinon = require('sinon')
import tape = require('tape')
import writeJsonFile = require('write-json-file')

const test = promisifyTape(tape)
const ncp = promisify(ncpCB.ncp)

test('uninstall package with no dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  let manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0'], await testDefaults({ save: true }))

  const reporter = sinon.spy()
  manifest = (await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ save: true, reporter })))[0].manifest

  t.ok(reporter.calledWithMatch({
    initial: {
      dependencies: {
        'is-negative': '2.1.0',
      },
    },
    level: 'debug',
    name: 'pnpm:package-manifest',
    prefix: process.cwd(),
  } as PackageManifestLog), 'initial package.json logged')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:stats',
    prefix: process.cwd(),
    removed: 1,
  } as StatsLog), 'reported info message about removing orphans')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'prod',
      name: 'is-negative',
      version: '2.1.0',
    },
  } as RootLog), 'removing root dependency reported')
  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:package-manifest',
    updated: {
      dependencies: {},
    },
  } as PackageManifestLog), 'updated package.json logged')

  // uninstall does not remove packages from store
  // even if they become unreferenced
  await project.storeHas('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  t.deepEqual(manifest.dependencies, {}, 'is-negative has been removed from dependencies')
})

test('uninstall a dependency that is not present in node_modules', async (t) => {
  prepareEmpty(t)

  const reporter = sinon.spy()
  await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest: {},
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      name: 'is-negative',
    },
  } as RootLog), 'removing root dependency reported')
})

test('uninstall scoped package', async (t) => {
  const project = prepareEmpty(t)
  let manifest = await addDependenciesToPackage({}, ['@zkochan/logger@0.1.0'], await testDefaults({ save: true }))
  manifest = (await mutateModules([
    {
      dependencyNames: ['@zkochan/logger'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ save: true })))[0].manifest

  await project.storeHas('@zkochan/logger', '0.1.0')

  await project.hasNot('@zkochan/logger')

  t.deepEqual(manifest.dependencies, {}, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults({ save: true })

  let manifest = await addDependenciesToPackage({}, [`http://localhost:${REGISTRY_MOCK_PORT}/is-array/-/is-array-1.0.1.tgz`], opts)
  manifest = (await mutateModules([
    {
      dependencyNames: ['is-array'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], opts))[0].manifest

  await project.storeHas('is-array', '1.0.1')
  await project.hasNot('is-array')

  t.deepEqual(manifest.dependencies, {}, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async (t) => {
  const project = prepareEmpty(t)
  let manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0', 'camelcase-keys@3.0.0'], await testDefaults({ save: true }))
  manifest = (await mutateModules([
    {
      dependencyNames: ['camelcase-keys'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ pruneStore: true, save: true })))[0].manifest

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHasNot('camelcase', '3.0.0')
  await project.hasNot('camelcase')

  await project.storeHasNot('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  t.deepEqual(manifest.dependencies, { 'is-negative': '2.1.0' }, 'camelcase-keys has been removed from dependencies')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.dependencies, {
    'is-negative': '2.1.0',
  }, 'camelcase-keys removed from lockfile dependencies')
  t.deepEqual(lockfile.specifiers, {
    'is-negative': '2.1.0',
  }, 'camelcase-keys removed from lockfile specifiers')
})

test('uninstall package with its bin files', async (t) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['sh-hello-world@1.0.1'], await testDefaults({ fastUnpack: false, save: true }))
  await mutateModules([
    {
      dependencyNames: ['sh-hello-world'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  const stat = await existsSymlink(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')

  t.notOk(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world')), 'sh-hello-world is removed from .bin')
  t.notOk(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world.cmd')), 'sh-hello-world.cmd is removed from .bin')
  t.notOk(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world.ps1')), 'sh-hello-world.ps1 is removed from .bin')
})

test('relative link is uninstalled', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const opts = await testDefaults({ manifest: {}, dir: process.cwd() })

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  const manifest = await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), opts as (typeof opts & { dir: string, manifest: PackageManifest }))
  await mutateModules([
    {
      dependencyNames: [linkedPkgName],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], opts)

  await project.hasNot(linkedPkgName)
})

test('pendingBuilds gets updated after uninstall', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example', 'with-postinstall-b'],
    await testDefaults({ fastUnpack: false, save: true, ignoreScripts: true })
  )

  const modules1 = await project.readModulesManifest()
  t.ok(modules1)
  t.equal(modules1!.pendingBuilds.length, 2, 'install should update pendingBuilds')

  await mutateModules([
    {
      dependencyNames: ['with-postinstall-b'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ save: true }))

  const modules2 = await project.readModulesManifest()
  t.ok(modules2)
  t.equal(modules2!.pendingBuilds.length, 1, 'uninstall should update pendingBuilds')
})

test('uninstalling a dependency from package that uses shared lockfile', async (t) => {
  const pkgs: PackageManifest[] = [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ]
  const projects = preparePackages(t, pkgs)

  const store = path.resolve('.store')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest: pkgs[0],
        mutation: 'install',
        rootDir: path.resolve('project-1'),
      },
      {
        buildIndex: 0,
        manifest: pkgs[1],
        mutation: 'install',
        rootDir: path.resolve('project-2'),
      },
    ],
    await testDefaults({
      store,
      workspacePackages: {
        'project-2': {
          '1.0.0': {
            dir: path.resolve('project-2'),
            manifest: {
              name: 'project-2',
              version: '1.0.0',

              dependencies: {
                'is-negative': '1.0.0',
              },
            },
          },
        },
      },
    })
  )

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  await mutateModules([
    {
      dependencyNames: ['is-positive', 'project-2'],
      manifest: pkgs[0],
      mutation: 'uninstallSome',
      rootDir: path.resolve('project-1'),
    },
  ], await testDefaults({
    lockfileDir: process.cwd(),
    store,
  }))

  await projects['project-1'].hasNot('is-positive')
  await projects['project-2'].has('is-negative')

  const lockfile = await readYamlFile<Lockfile>(WANTED_LOCKFILE)

  t.deepEqual(lockfile, {
    importers: {
      'project-1': {
        specifiers: {},
      },
      'project-2': {
        dependencies: {
          'is-negative': '1.0.0',
        },
        specifiers: {
          'is-negative': '1.0.0',
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-clmHeoPIAKwxkd17nZ+80PdS1P4=',
        },
      },
    },
  })
})

test('uninstall remove modules that is not in package.json', async (t) => {
  const project = prepareEmpty(t)

  await writeJsonFile('node_modules/foo/package.json', { name: 'foo', version: '1.0.0' })

  await project.has('foo')

  await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        manifest: {},
        mutation: 'uninstallSome',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults()
  )

  await project.hasNot('foo')
})
