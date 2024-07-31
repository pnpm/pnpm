import fs from 'fs'
import path from 'path'
import { assertProject } from '@pnpm/assert-project'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  type MutatedProject,
  mutateModules,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { type DepPath, type ProjectRootDir } from '@pnpm/types'
import { sync as rimraf } from '@zkochan/rimraf'
import resolveLinkTarget from 'resolve-link-target'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { addDistTag } from '@pnpm/registry-mock'
import symlinkDir from 'symlink-dir'
import { sync as writeYamlFile } from 'write-yaml-file'
import { testDefaults } from '../utils'

test('should hoist dependencies', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['express', '@foo/has-dep-from-same-scope'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  project.has('express')
  project.has('.pnpm/node_modules/debug')
  project.has('.pnpm/node_modules/cookie')
  project.has('.pnpm/node_modules/mime')
  project.has('@foo/has-dep-from-same-scope')
  project.has('.pnpm/node_modules/@foo/no-deps')

  // should also hoist bins
  project.isExecutable('.pnpm/node_modules/.bin/mime')

  const modules = project.readModulesManifest()
  expect(Object.keys(modules!.hoistedDependencies).length > 0).toBeTruthy()

  // On repeat install the hoisted packages are preserved (non-headless install)
  await install(manifest, testDefaults({ fastUnpack: false, hoistPattern: '*', preferFrozenLockfile: false, modulesCacheMaxAge: 0 }))
  project.has('.pnpm/node_modules/debug')
  expect((project.readModulesManifest())!.hoistedDependencies).toStrictEqual(modules!.hoistedDependencies)

  // On repeat install the hoisted packages are preserved (headless install)
  await install(manifest, testDefaults({ fastUnpack: false, hoistPattern: '*', frozenLockfile: true, modulesCacheMaxAge: 0 }))
  project.has('.pnpm/node_modules/debug')
  expect((project.readModulesManifest())!.hoistedDependencies).toStrictEqual(modules!.hoistedDependencies)
})

test('should hoist dependencies to the root of node_modules when publicHoistPattern is used', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    ['express', '@foo/has-dep-from-same-scope'],
    testDefaults({ fastUnpack: false, publicHoistPattern: '*' }))

  project.has('express')
  project.has('debug')
  project.has('cookie')
  project.has('mime')
  project.has('@foo/has-dep-from-same-scope')
  project.has('@foo/no-deps')

  // should also hoist bins
  project.isExecutable('.bin/mime')
})

test('public hoist should not override directories that are already in the root of node_modules', async () => {
  const project = prepareEmpty()
  fs.mkdirSync('node_modules/debug', { recursive: true })
  fs.writeFileSync('node_modules/debug/pnpm-test.txt', '')
  fs.mkdirSync('cookie')
  fs.writeFileSync('cookie/pnpm-test.txt', '')
  await symlinkDir('cookie', 'node_modules/cookie')

  await addDependenciesToPackage({},
    ['express@4.18.2'],
    testDefaults({ fastUnpack: false, publicHoistPattern: '*' }))

  project.has('express')
  project.has('debug/pnpm-test.txt')
  project.has('cookie/pnpm-test.txt')
  project.has('mime')
})

test('should hoist some dependencies to the root of node_modules when publicHoistPattern is used and others to the virtual store directory', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    ['express', '@foo/has-dep-from-same-scope'],
    testDefaults({ fastUnpack: false, hoistPattern: '*', publicHoistPattern: '@foo/*' }))

  project.has('express')
  project.has('.pnpm/node_modules/debug')
  project.has('.pnpm/node_modules/cookie')
  project.has('.pnpm/node_modules/mime')
  project.has('@foo/has-dep-from-same-scope')
  project.has('@foo/no-deps')

  // should also hoist bins
  project.isExecutable('.pnpm/node_modules/.bin/mime')
})

test('should hoist dependencies by pattern', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['express'], testDefaults({ fastUnpack: false, hoistPattern: 'mime' }))

  project.has('express')
  project.hasNot('.pnpm/node_modules/debug')
  project.hasNot('.pnpm/node_modules/cookie')
  project.has('.pnpm/node_modules/mime')

  // should also hoist bins
  project.isExecutable('.pnpm/node_modules/.bin/mime')
})

test('should remove hoisted dependencies', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['express'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))
  await mutateModulesInSingleProject({
    dependencyNames: ['express'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  project.hasNot('express')
  project.hasNot('.pnpm/node_modules/debug')
  project.hasNot('.pnpm/node_modules/cookie')
})

test('should not override root packages with hoisted dependencies', async () => {
  const project = prepareEmpty()

  // this installs debug@3.1.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0'], testDefaults({ hoistPattern: '*' }))
  // this installs express@4.16.2, that depends on debug 2.6.9, but we don't want to flatten debug@2.6.9
  await addDependenciesToPackage(manifest, ['express@4.16.2'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
})

test('should rehoist when uninstalling a package', async () => {
  const project = prepareEmpty()

  // this installs debug@3.1.0 and express@4.16.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0', 'express@4.16.0'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))
  // uninstall debug@3.1.0 to check if debug@2.6.9 gets reflattened
  await mutateModulesInSingleProject({
    dependencyNames: ['debug'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  expect(project.requireModule('.pnpm/node_modules/debug/package.json').version).toEqual('2.6.9')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  const modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies['debug@2.6.9' as DepPath]).toStrictEqual({ debug: 'private' })
})

test('should rehoist after running a general install', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  project.hasNot('.pnpm/node_modules/debug') // debug not hoisted because it is a direct dep

  // read this module path because we can't use requireModule again, as it is cached
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that express stays at the same version
  await install({
    dependencies: {
      express: '4.16.0',
    },
  }, testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  expect(prevExpressModulePath).toEqual(currExpressModulePath)

  project.has('.pnpm/node_modules/debug') // debug hoisted because it is not a direct dep anymore
})

test('should not override aliased dependencies', async () => {
  const project = prepareEmpty()
  // now I install is-negative, but aliased as "debug". I do not want the "debug" dependency of express to override my alias
  await addDependenciesToPackage({}, ['debug@npm:is-negative@1.0.0', 'express'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('1.0.0')
})

test('hoistPattern=* throws exception when executed on node_modules installed w/o the option', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive'], testDefaults({ hoistPattern: undefined }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], testDefaults({
      forceHoistPattern: true,
      hoistPattern: '*',
    }))
  ).rejects.toThrow(/different hoist-pattern value/)
})

test('hoistPattern=undefined throws exception when executed on node_modules installed with hoist-pattern=*', async () => {
  prepareEmpty()
  const opts = testDefaults({ hoistPattern: '*' })
  const manifest = await addDependenciesToPackage({}, ['is-positive'], opts)

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], {
      ...opts,
      forceHoistPattern: true,
      hoistPattern: undefined,
    })
  ).rejects.toThrow(/different hoist-pattern value/)

  // Install doesn't fail if the value of hoistPattern isn't forced
  await addDependenciesToPackage(manifest, ['is-negative'], {
    ...opts,
    forceHoistPattern: false,
    hoistPattern: undefined,
  })
})

test('hoist by alias', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  // pkg-with-1-aliased-dep aliases @pnpm.e2e/dep-of-pkg-with-1-dep as just "dep"
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-aliased-dep'], testDefaults({ hoistPattern: '*' }))

  project.has('@pnpm.e2e/pkg-with-1-aliased-dep')
  project.has('.pnpm/node_modules/dep')
  project.hasNot('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  const modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies).toStrictEqual({ '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': { dep: 'private' } })
})

test('should remove aliased hoisted dependencies', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-aliased-dep'], testDefaults({ hoistPattern: '*' }))
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/pkg-with-1-aliased-dep'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  project.hasNot('@pnpm.e2e/pkg-with-1-aliased-dep')
  project.hasNot('@pnpm.e2e/dep-of-pkg-with-1-dep')
  try {
    await resolveLinkTarget('./node_modules/dep')
    throw new Error('should have failed')
  } catch (e) {}

  const modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies).toStrictEqual({})
})

test('should update .modules.yaml when pruning if we are flattening', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-aliased-dep': '*',
    },
  }, testDefaults({ hoistPattern: '*' }))

  await install({}, testDefaults({ hoistPattern: '*', pruneStore: true }))

  const modules = project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies).toStrictEqual({})
})

test('should rehoist after pruning', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  project.hasNot('.pnpm/node_modules/debug') // debug is not hoisted because it is a direct dep
  // read this module path because we can't use requireModule again, as it is cached
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that ms is still there, and that is-positive is not installed
  await install({
    dependencies: {
      express: '4.16.0',
      'is-positive': '1.0.0',
    },
  }, testDefaults({ fastUnpack: false, hoistPattern: '*', pruneStore: true }))

  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  expect(prevExpressModulePath).toEqual(currExpressModulePath)

  project.has('.pnpm/node_modules/debug') // debug is hoisted because it is not a direct dep anymore
})

test('should hoist correctly peer dependencies', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/using-ajv'], testDefaults({ hoistPattern: '*' }))

  project.has('.pnpm/node_modules/ajv-keywords')
})

test('should uninstall correctly peer dependencies', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/using-ajv'], testDefaults({ hoistPattern: '*' }))
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/using-ajv'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  // symlink to peer dependency is deleted
  expect(() => fs.lstatSync('node_modules/ajv-keywords')).toThrow()
})

test('hoist-pattern: hoist all dependencies to the virtual store node_modules', async () => {
  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',

    dependencies: {
      '@pnpm.e2e/foobar': '100.0.0',
    },
  }
  const projects = preparePackages([
    {
      location: '.',
      package: workspaceRootManifest,
    },
    {
      location: 'package',
      package: workspacePackageManifest,
    },
  ])

  const mutatedProjects: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('package') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: workspaceRootManifest,
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      rootDir: path.resolve('package') as ProjectRootDir,
    },
  ]
  await mutateModules(mutatedProjects, testDefaults({ allProjects, hoistPattern: '*' }))

  projects['root'].has('@pnpm.e2e/pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foobar')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foo')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/bar')
  projects['root'].hasNot('@pnpm.e2e/foobar')
  projects['root'].hasNot('@pnpm.e2e/foo')
  projects['root'].hasNot('@pnpm.e2e/bar')

  projects['package'].has('@pnpm.e2e/foobar')
  projects['package'].hasNot('@pnpm.e2e/foo')
  projects['package'].hasNot('@pnpm.e2e/bar')

  rimraf('node_modules')
  rimraf('package/node_modules')

  await mutateModules(mutatedProjects, testDefaults({ allProjects, frozenLockfile: true, hoistPattern: '*' }))

  projects['root'].has('@pnpm.e2e/pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foobar')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foo')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/bar')
  projects['root'].hasNot('@pnpm.e2e/foobar')
  projects['root'].hasNot('@pnpm.e2e/foo')
  projects['root'].hasNot('@pnpm.e2e/bar')

  projects['package'].has('@pnpm.e2e/foobar')
  projects['package'].hasNot('@pnpm.e2e/foo')
  projects['package'].hasNot('@pnpm.e2e/bar')
})

test('hoist when updating in one of the workspace projects', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: 'latest' })

  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',

    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  }
  preparePackages([
    {
      location: '.',
      package: workspaceRootManifest,
    },
    {
      location: 'package',
      package: workspacePackageManifest,
    },
  ])

  const mutatedProjects: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('package') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: workspaceRootManifest,
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      rootDir: path.resolve('package') as ProjectRootDir,
    },
  ]
  await mutateModules(mutatedProjects, testDefaults({ allProjects, hoistPattern: '*' }))

  const rootModules = assertProject(process.cwd())
  {
    const modulesManifest = rootModules.readModulesManifest()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': { '@pnpm.e2e/dep-of-pkg-with-1-dep': 'private' },
      '@pnpm.e2e/foo@100.0.0': { '@pnpm.e2e/foo': 'private' },
    })
  }

  await mutateModules([
    {
      ...mutatedProjects[0],
      dependencySelectors: ['@pnpm.e2e/foo@100.1.0'],
      mutation: 'installSome',
    },
  ], testDefaults({ allProjects, hoistPattern: '*', pruneLockfileImporters: false }))

  const lockfile = rootModules.readCurrentLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
      '@pnpm.e2e/foo@100.0.0',
      '@pnpm.e2e/foo@100.1.0',
      '@pnpm.e2e/pkg-with-1-dep@100.0.0',
    ]
  )

  {
    const modulesManifest = rootModules.readModulesManifest()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0': { '@pnpm.e2e/dep-of-pkg-with-1-dep': 'private' },
    })
  }
})

test('should recreate node_modules with hoisting', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({ hoistPattern: undefined }))

  project.hasNot('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  {
    const modulesManifest = project.readModulesManifest()
    expect(modulesManifest?.hoistPattern).toBeFalsy()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({})
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  project.has('@pnpm.e2e/pkg-with-1-dep')
  project.has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  {
    const modulesManifest = project.readModulesManifest()
    expect(modulesManifest?.hoistPattern).toBeTruthy()
    expect(Object.keys(modulesManifest?.hoistedDependencies ?? {}).length > 0).toBeTruthy()
  }
})

test('hoisting should not create a broken symlink to a skipped optional dependency', async () => {
  const project = prepareEmpty()
  const rootModules = assertProject(process.cwd())

  const manifest = {
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  }

  await install(manifest, testDefaults({ publicHoistPattern: '*' }))

  project.hasNot('@pnpm.e2e/dep-of-optional-pkg')
  expect(rootModules.readCurrentLockfile()).toStrictEqual(rootModules.readLockfile())

  // Verifying the same with headless installation
  rimraf('node_modules')

  await install(manifest, testDefaults({ publicHoistPattern: '*' }))

  project.hasNot('@pnpm.e2e/dep-of-optional-pkg')
  expect(rootModules.readCurrentLockfile()).toStrictEqual(rootModules.readLockfile())
})

test('the hoisted packages should not override the bin files of the direct dependencies', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin-parent'], testDefaults({ fastUnpack: false, publicHoistPattern: '*' }))

  {
    const cmd = await fs.promises.readFile('node_modules/.bin/hello-world-js-bin', 'utf-8')
    expect(cmd).toContain('/hello-world-js-bin-parent/')
  }

  rimraf('node_modules')

  await install(manifest, testDefaults({ fastUnpack: false, frozenLockfile: true, publicHoistPattern: '*' }))

  {
    const cmd = await fs.promises.readFile('node_modules/.bin/hello-world-js-bin', 'utf-8')
    expect(cmd).toContain('/hello-world-js-bin-parent/')
  }
})

test('hoist packages which is in the dependencies tree of the selected projects', async () => {
  const { root } = preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-1',
      package: { name: 'project-1', dependencies: { 'is-positive': '2.0.0' } },
    },
    {
      location: 'project-2',
      package: { name: 'project-2', dependencies: { 'is-positive': '3.0.0' } },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('.') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'root',
        version: '1.0.0',
      },
      rootDir: path.resolve('.') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          'is-positive': '3.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  /**
   * project-1 locks is-positive@2.0.0 while project-2 locks is-positive@3.0.0
   * when partial install project@3.0.0, is-positive@3.0.0 always should be hoisted
   * instead of using is-positive@2.0.0 and does not hoist anything
   */
  writeYamlFile(WANTED_LOCKFILE, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {},
      'project-1': {
        dependencies: {
          'is-positive': {
            specifier: '2.0.0',
            version: '2.0.0',
          },
        },
      },
      'project-2': {
        dependencies: {
          'is-positive': {
            specifier: '3.0.0',
            version: '3.0.0',
          },
        },
      },
    },
    packages: {
      'is-positive@2.0.0': {
        resolution: { integrity: 'sha1-sU8GvS24EK5sixJ0HRNr+u8Nh70=' },
        engines: { node: '>=0.10.0' },
        dev: false,
      },
      'is-positive@3.0.0': {
        resolution: { integrity: 'sha1-jvDuIvfOJPdjP4kIAw7Ei2Ks9KM=' },
        engines: { node: '>=0.10.0' },
        dev: false,
      },
    },
  }, { lineWidth: 1000 })

  await mutateModules(importers, testDefaults({ allProjects, hoistPattern: '*' }))

  root.has('.pnpm/node_modules/is-positive')
  const { version } = root.requireModule('.pnpm/node_modules/is-positive/package.json')
  expect(version).toBe('3.0.0')
})

test('only hoist packages which is in the dependencies tree of the selected projects with sub dependencies', async () => {
  const { root } = preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-1',
      package: { name: 'project-1', dependencies: { '@babel/runtime-corejs3': '7.15.3' } },
    },
    {
      location: 'project-2',
      package: { name: 'project-2', dependencies: { '@babel/runtime-corejs3': '7.15.4' } },
    },
  ])

  writeYamlFile(WANTED_LOCKFILE, {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {},
      'project-1': {
        dependencies: {
          '@babel/runtime-corejs3': {
            specifier: '7.15.3',
            version: '7.15.3',
          },
        },
      },
      'project-2': {
        dependencies: {
          '@babel/runtime-corejs3': {
            specifier: '7.15.4',
            version: '7.15.4',
          },
        },
      },
    },
    packages: {
      '@babel/runtime-corejs3@7.15.3': {
        resolution: { integrity: 'sha512-30A3lP+sRL6ml8uhoJSs+8jwpKzbw8CqBvDc1laeptxPm5FahumJxirigcbD2qTs71Sonvj1cyZB0OKGAmxQ+A==' },
        engines: { node: '>=6.9.0' },
      },
      '@babel/runtime-corejs3@7.15.4': {
        resolution: { integrity: 'sha512-lWcAqKeB624/twtTc3w6w/2o9RqJPaNBhPGK6DKLSiwuVWC7WFkypWyNg+CpZoyJH0jVzv1uMtXZ/5/lQOLtCg==' },
        engines: { node: '>=6.9.0' },
      },
      'core-js-pure@3.17.2': {
        resolution: { integrity: 'sha512-2VV7DlIbooyTI7Bh+yzOOWL9tGwLnQKHno7qATE+fqZzDKYr6llVjVQOzpD/QLZFgXDPb8T71pJokHEZHEYJhQ==' },
      },
      'core-js-pure@3.17.3': {
        resolution: { integrity: 'sha512-YusrqwiOTTn8058JDa0cv9unbXdIiIgcgI9gXso0ey4WgkFLd3lYlV9rp9n7nDCsYxXsMDTjA4m1h3T348mdlQ==' },
      },
      'regenerator-runtime@0.13.9': {
        resolution: { integrity: 'sha512-p3VT+cOEgxFsRRA9X4lkI1E+k2/CtnKtU4gcxyaCUreilL/vqI6CdZ3wxVUx3UOUg+gnUOQQcRI7BmSI656MYA==' },
      },
    },
    snapshots: {
      '@babel/runtime-corejs3@7.15.3': {
        dependencies: {
          'core-js-pure': '3.17.2',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      '@babel/runtime-corejs3@7.15.4': {
        dependencies: {
          'core-js-pure': '3.17.3',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      'core-js-pure@3.17.2': {
        dev: false,
      },
      'core-js-pure@3.17.3': {
        dev: false,
      },
      'regenerator-runtime@0.13.9': {
        dev: false,
      },
    },
  }, { lineWidth: 1000 })

  await mutateModulesInSingleProject({
    manifest: {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        '@babel/runtime-corejs3': '7.15.4',
      },
    },
    mutation: 'install',
    rootDir: path.resolve('project-2') as ProjectRootDir,
  }, testDefaults({ hoistPattern: '*' }))

  root.has('.pnpm/node_modules/@babel/runtime-corejs3')
  const { version: runtimeVersion } = root.requireModule('.pnpm/node_modules/@babel/runtime-corejs3/package.json')
  expect(runtimeVersion).toBe('7.15.4')

  root.has('.pnpm/node_modules/core-js-pure')
  const { version: coreJsVersion } = root.requireModule('.pnpm/node_modules/core-js-pure/package.json')
  expect(coreJsVersion).toBe('3.17.3')

  root.has('.pnpm/node_modules/regenerator-runtime')
  const { version: regeneratorVersion } = root.requireModule('.pnpm/node_modules/regenerator-runtime/package.json')
  expect(regeneratorVersion).toBe('0.13.9')
})

test('should add extra node paths to command shims', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin'], testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  const cmdShim = fs.readFileSync(path.join('node_modules', '.bin', 'hello-world-js-bin'), 'utf8')
  expect(cmdShim).toContain('node_modules/.pnpm/node_modules')
})

test('should not add extra node paths to command shims, when extend-node-path is set to false', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/hello-world-js-bin'], testDefaults({
    fastUnpack: false,
    extendNodePath: false,
    hoistPattern: '*',
  }))

  const cmdShim = fs.readFileSync(path.join('node_modules', '.bin', 'hello-world-js-bin'), 'utf8')
  console.log(cmdShim)
  expect(cmdShim).not.toContain('node_modules/.pnpm/node_modules')
})

test('hoistWorkspacePackages should hoist all workspace projects', async () => {
  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',
    version: '1.0.0',

    dependencies: {
      '@pnpm.e2e/foobar': '100.0.0',
    },
  }
  const workspacePackageManifest2 = {
    name: 'package2',
    version: '1.0.0',

    dependencies: {
      package: 'workspace:*',
    },
  }

  const projects = preparePackages([
    {
      location: '.',
      package: workspaceRootManifest,
    },
    {
      location: 'package',
      package: workspacePackageManifest,
    },
    {
      location: 'package2',
      package: workspacePackageManifest2,
    },
  ])

  const mutatedProjects: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('package') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('package2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: workspaceRootManifest,
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      rootDir: path.resolve('package') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest2,
      rootDir: path.resolve('package2') as ProjectRootDir,
    },
  ]
  await mutateModules(mutatedProjects, testDefaults({
    allProjects,
    hoistPattern: '*',
    hoistWorkspacePackages: true,
  }))

  projects['root'].has('@pnpm.e2e/pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foobar')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/foo')
  projects['root'].has('.pnpm/node_modules/@pnpm.e2e/bar')
  projects['root'].has('.pnpm/node_modules/package')
  projects['root'].has('.pnpm/node_modules/package2')
  projects['root'].hasNot('.pnpm/node_modules/root')
  projects['root'].hasNot('@pnpm.e2e/foobar')
  projects['root'].hasNot('@pnpm.e2e/foo')
  projects['root'].hasNot('@pnpm.e2e/bar')

  projects['package'].has('@pnpm.e2e/foobar')
  projects['package'].hasNot('@pnpm.e2e/foo')
  projects['package'].hasNot('@pnpm.e2e/bar')

  rimraf('node_modules')
  await mutateModules(mutatedProjects, testDefaults({
    allProjects,
    frozenLockfile: true,
    hoistPattern: '*',
    hoistWorkspacePackages: true,
  }))
  projects['root'].has('.pnpm/node_modules/package')
  projects['root'].has('.pnpm/node_modules/package2')
  projects['root'].hasNot('.pnpm/node_modules/root')
})
