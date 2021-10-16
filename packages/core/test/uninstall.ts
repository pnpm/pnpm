import { promisify } from 'util'
import path from 'path'
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
} from '@pnpm/core'
import exists from 'path-exists'
import sinon from 'sinon'
import writeJsonFile from 'write-json-file'
import existsSymlink from 'exists-link'
import ncpCB from 'ncp'
import { testDefaults } from './utils'

const ncp = promisify(ncpCB.ncp)

test('uninstall package with no dependencies', async () => {
  const project = prepareEmpty()

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
  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: 'prod',
      name: 'is-negative',
      version: '2.1.0',
    },
  } as RootLog)).toBeTruthy()
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

test('uninstall a dependency that is not present in node_modules', async () => {
  prepareEmpty()

  const reporter = sinon.spy()
  await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest: {},
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      name: 'is-negative',
    },
  } as RootLog)).toBeFalsy()
})

test('uninstall scoped package', async () => {
  const project = prepareEmpty()
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

  expect(manifest.dependencies).toStrictEqual({})
})

test('uninstall tarball dependency', async () => {
  const project = prepareEmpty()
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

  expect(manifest.dependencies).toStrictEqual({})
})

test('uninstall package with dependencies and do not touch other deps', async () => {
  const project = prepareEmpty()
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

  expect(manifest.dependencies).toStrictEqual({ 'is-negative': '2.1.0' })

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies).toStrictEqual({
    'is-negative': '2.1.0',
  })
  expect(lockfile.specifiers).toStrictEqual({
    'is-negative': '2.1.0',
  })
})

test('uninstall package with its bin files', async () => {
  prepareEmpty()
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
  expect(stat).toBeFalsy()

  expect(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world.cmd'))).toBeFalsy()
  expect(await exists(path.resolve('node_modules', '.bin', 'sh-hello-world.ps1'))).toBeFalsy()
})

test('relative link is uninstalled', async () => {
  const project = prepareEmpty()
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

test('pendingBuilds gets updated after uninstall', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({},
    ['pre-and-postinstall-scripts-example', 'with-postinstall-b'],
    await testDefaults({ fastUnpack: false, save: true, ignoreScripts: true })
  )

  const modules1 = await project.readModulesManifest()
  expect(modules1).toBeTruthy()
  expect(modules1!.pendingBuilds.length).toBe(2)

  await mutateModules([
    {
      dependencyNames: ['with-postinstall-b'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ save: true }))

  const modules2 = await project.readModulesManifest()
  expect(modules2).toBeTruthy()
  expect(modules2!.pendingBuilds.length).toBe(1)
})

test('uninstalling a dependency from package that uses shared lockfile', async () => {
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
  const projects = preparePackages(pkgs)

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

  expect(lockfile).toStrictEqual({
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

test('uninstall remove modules that is not in package.json', async () => {
  const project = prepareEmpty()

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
