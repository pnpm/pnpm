import {
  PackageJsonLog,
  RootLog,
  StatsLog,
} from '@pnpm/core-loggers'
import prepare, { preparePackages } from '@pnpm/prepare'
import { Shrinkwrap } from '@pnpm/shrinkwrap-file'
import existsSymlink = require('exists-link')
import ncpCB = require('ncp')
import path = require('path')
import exists = require('path-exists')
import readPkg = require('read-pkg')
import readYamlFile from 'read-yaml-file'
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  link,
  mutateModules,
  storePrune,
  uninstall,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import promisify = require('util.promisify')
import writeJsonFile from 'write-json-file'
import {
  pathToLocalPkg,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
const ncp = promisify(ncpCB.ncp)

test('uninstall package with no dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults({ save: true }))

  const reporter = sinon.spy()
  await uninstall(['is-negative'], await testDefaults({ save: true, reporter }))

  t.ok(reporter.calledWithMatch({
    initial: {
      dependencies: {
        'is-negative': '^2.1.0',
      },
      name: 'project',
      version: '0.0.0',
    },
    level: 'debug',
    name: 'pnpm:package-json',
    prefix: process.cwd(),
  } as PackageJsonLog), 'initial package.json logged')
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
    name: 'pnpm:package-json',
    updated: {
      name: 'project',
      version: '0.0.0',
    },
  } as PackageJsonLog), 'updated package.json logged')

  // uninstall does not remove packages from store
  // even if they become unreferenced
  await project.storeHas('is-negative', '2.1.0')

  await project.hasNot('is-negative')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-negative has been removed from dependencies')
})

test('uninstall a dependency that is not present in node_modules', async (t) => {
  const project = prepare(t)

  const reporter = sinon.spy()
  await uninstall(['is-negative'], await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      name: 'is-negative',
    },
  } as RootLog), 'removing root dependency reported')
})

test('uninstall scoped package', async (t) => {
  const project = prepare(t)
  await addDependenciesToPackage(['@zkochan/logger@0.1.0'], await testDefaults({ save: true }))
  await uninstall(['@zkochan/logger'], await testDefaults({ save: true }))

  await project.storeHas('@zkochan/logger', '0.1.0')

  await project.hasNot('@zkochan/logger')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, '@zkochan/logger has been removed from dependencies')
})

test('uninstall tarball dependency', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults({ save: true })
  await addDependenciesToPackage(['http://localhost:4873/is-array/-/is-array-1.0.1.tgz'], opts)
  await uninstall(['is-array'], opts)

  t.ok(await exists(path.join(await project.getStorePath(), 'localhost+4873', 'is-array', '1.0.1')))

  await project.hasNot('is-array')

  const pkgJson = await readPkg()
  t.equal(pkgJson.dependencies, undefined, 'is-array has been removed from dependencies')
})

test('uninstall package with dependencies and do not touch other deps', async (t) => {
  const project = prepare(t)
  await addDependenciesToPackage(['is-negative@2.1.0', 'camelcase-keys@3.0.0'], await testDefaults({ save: true }))
  await uninstall(['camelcase-keys'], await testDefaults({ save: true }))

  await storePrune(await testDefaults())

  await project.storeHasNot('camelcase-keys', '3.0.0')
  await project.hasNot('camelcase-keys')

  await project.storeHasNot('camelcase', '3.0.0')
  await project.hasNot('camelcase')

  await project.storeHasNot('map-obj', '1.0.1')
  await project.hasNot('map-obj')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, { 'is-negative': '^2.1.0' }, 'camelcase-keys has been removed from dependencies')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.dependencies, {
    'is-negative': '2.1.0',
  }, 'camelcase-keys removed from shrinkwrap dependencies')
  t.deepEqual(shr.specifiers, {
    'is-negative': '^2.1.0',
  }, 'camelcase-keys removed from shrinkwrap specifiers')
})

test('uninstall package with its bin files', async (t) => {
  prepare(t)
  await addDependenciesToPackage(['sh-hello-world@1.0.1'], await testDefaults({ save: true }))
  await uninstall(['sh-hello-world'], await testDefaults({ save: true }))

  // check for both a symlink and a file because in some cases the file will be a proxied not symlinked
  let stat = await existsSymlink(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')

  stat = await exists(path.resolve('node_modules', '.bin', 'sh-hello-world'))
  t.notOk(stat, 'sh-hello-world is removed from .bin')
})

test('relative link is uninstalled', async (t: tape.Test) => {
  const project = prepare(t)
  const opts = await testDefaults()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([`../${linkedPkgName}`], path.join(process.cwd(), 'node_modules'), opts)
  await uninstall([linkedPkgName], opts)

  await project.hasNot(linkedPkgName)
})

test('pendingBuilds gets updated after uninstall', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['pre-and-postinstall-scripts-example', 'with-postinstall-b'], await testDefaults({ save: true, ignoreScripts: true }))

  const modules1 = await project.loadModules()
  t.ok(modules1)
  t.equal(modules1!.pendingBuilds.length, 2, 'install should update pendingBuilds')

  await uninstall(['with-postinstall-b'], await testDefaults({ save: true }))

  const modules2 = await project.loadModules()
  t.ok(modules2)
  t.equal(modules2!.pendingBuilds.length, 1, 'uninstall should update pendingBuilds')
})

test('uninstalling a dependency from package that uses shared shrinkwrap', async (t) => {
  const projects = preparePackages(t, [
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
  ])

  const store = path.resolve('.store')

  await mutateModules(
    [
      {
        mutation: 'install',
        prefix: path.resolve('project-1'),
      },
      {
        mutation: 'install',
        prefix: path.resolve('project-2'),
      },
    ],
    await testDefaults({
      localPackages: {
        'project-2': {
          '1.0.0': {
            directory: path.resolve('project-2'),
            package: {
              name: 'project-2',
              version: '1.0.0',

              dependencies: {
                'is-negative': '1.0.0',
              },
            },
          },
        },
      },
      store,
    }),
  )

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')

  await uninstall(['is-positive', 'project-2'], await testDefaults({
    prefix: path.resolve('project-1'),
    shrinkwrapDirectory: process.cwd(),
    store,
  }))

  await projects['project-1'].hasNot('is-positive')
  await projects['project-2'].has('is-negative')

  const shr = await readYamlFile<Shrinkwrap>('shrinkwrap.yaml')

  t.deepEqual(shr, {
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
    shrinkwrapVersion: 4,
  })
})

test('uninstall remove modules that is not in package.json', async (t) => {
  const project = prepare(t)

  await writeJsonFile('node_modules/foo/package.json', { name: 'foo', version: '1.0.0' })

  await project.has('foo')

  await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        mutation: 'uninstallSome',
        prefix: process.cwd(),
      },
    ],
    await testDefaults(),
  )

  await project.hasNot('foo')
})
