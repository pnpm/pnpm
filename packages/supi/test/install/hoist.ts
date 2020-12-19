import assertProject from '@pnpm/assert-project'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  MutatedProject,
  mutateModules,
} from 'supi'
import { addDistTag, testDefaults } from '../utils'
import fs = require('fs')
import rimraf = require('@zkochan/rimraf')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')

test('should hoist dependencies', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['express', '@foo/has-dep-from-same-scope'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  await project.has('express')
  await project.has('.pnpm/node_modules/debug')
  await project.has('.pnpm/node_modules/cookie')
  await project.has('.pnpm/node_modules/mime')
  await project.has('@foo/has-dep-from-same-scope')
  await project.has('.pnpm/node_modules/@foo/no-deps')

  // should also hoist bins
  await project.isExecutable('.pnpm/node_modules/.bin/mime')
})

test('should hoist dependencies to the root of node_modules when publicHoistPattern is used', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    ['express', '@foo/has-dep-from-same-scope'],
    await testDefaults({ fastUnpack: false, publicHoistPattern: '*' }))

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')
  await project.has('mime')
  await project.has('@foo/has-dep-from-same-scope')
  await project.has('@foo/no-deps')

  // should also hoist bins
  await project.isExecutable('.bin/mime')
})

test('should hoist some dependencies to the root of node_modules when publicHoistPattern is used and others to the virtual store directory', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({},
    ['express', '@foo/has-dep-from-same-scope'],
    await testDefaults({ fastUnpack: false, hoistPattern: '*', publicHoistPattern: '@foo/*' }))

  await project.has('express')
  await project.has('.pnpm/node_modules/debug')
  await project.has('.pnpm/node_modules/cookie')
  await project.has('.pnpm/node_modules/mime')
  await project.has('@foo/has-dep-from-same-scope')
  await project.has('@foo/no-deps')

  // should also hoist bins
  await project.isExecutable('.pnpm/node_modules/.bin/mime')
})

test('should hoist dependencies by pattern', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['express'], await testDefaults({ fastUnpack: false, hoistPattern: 'mime' }))

  await project.has('express')
  await project.hasNot('.pnpm/node_modules/debug')
  await project.hasNot('.pnpm/node_modules/cookie')
  await project.has('.pnpm/node_modules/mime')

  // should also hoist bins
  await project.isExecutable('.pnpm/node_modules/.bin/mime')
})

test('should remove hoisted dependencies', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['express'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['express'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  await project.hasNot('express')
  await project.hasNot('.pnpm/node_modules/debug')
  await project.hasNot('.pnpm/node_modules/cookie')
})

test('should not override root packages with hoisted dependencies', async () => {
  const project = prepareEmpty()

  // this installs debug@3.1.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0'], await testDefaults({ hoistPattern: '*' }))
  // this installs express@4.16.2, that depends on debug 2.6.9, but we don't want to flatten debug@2.6.9
  await addDependenciesToPackage(manifest, ['express@4.16.2'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
})

test('should rehoist when uninstalling a package', async () => {
  const project = prepareEmpty()

  // this installs debug@3.1.0 and express@4.16.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0', 'express@4.16.0'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))
  // uninstall debug@3.1.0 to check if debug@2.6.9 gets reflattened
  await mutateModules([
    {
      dependencyNames: ['debug'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  expect(project.requireModule('.pnpm/node_modules/debug/package.json').version).toEqual('2.6.9')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  const modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies['/debug/2.6.9']).toStrictEqual({ debug: 'private' })
})

test('should rehoist after running a general install', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  await project.hasNot('.pnpm/node_modules/debug') // debug not hoisted because it is a direct dep

  // read this module path because we can't use requireModule again, as it is cached
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that express stays at the same version
  await install({
    dependencies: {
      express: '4.16.0',
    },
  }, await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  expect(prevExpressModulePath).toEqual(currExpressModulePath)

  await project.has('.pnpm/node_modules/debug') // debug hoisted because it is not a direct dep anymore
})

test('should not override aliased dependencies', async () => {
  const project = prepareEmpty()
  // now I install is-negative, but aliased as "debug". I do not want the "debug" dependency of express to override my alias
  await addDependenciesToPackage({}, ['debug@npm:is-negative@1.0.0', 'express'], await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('1.0.0')
})

test('hoistPattern=* throws exception when executed on node_modules installed w/o the option', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ hoistPattern: undefined }))

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({
      forceHoistPattern: true,
      hoistPattern: '*',
    }))
  ).rejects.toThrow(/different hoist-pattern value/)
})

test('hoistPattern=undefined throws exception when executed on node_modules installed with hoist-pattern=*', async () => {
  prepareEmpty()
  const opts = await testDefaults({ hoistPattern: '*' })
  const manifest = await addDependenciesToPackage({}, ['is-positive'], opts)

  await expect(
    addDependenciesToPackage(manifest, ['is-negative'], {
      ...opts,
      forceHoistPattern: true,
      hoistPattern: undefined,
    })
  ).rejects.toThrow(/different hoist-pattern value/)

  // Instatll doesn't fail if the value of hoistPattern isn't forced
  await addDependenciesToPackage(manifest, ['is-negative'], {
    ...opts,
    forceHoistPattern: false,
    hoistPattern: undefined,
  })
})

test('hoist by alias', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  const project = prepareEmpty()

  // pkg-with-1-aliased-dep aliases dep-of-pkg-with-1-dep as just "dep"
  await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ hoistPattern: '*' }))

  await project.has('pkg-with-1-aliased-dep')
  await project.has('.pnpm/node_modules/dep')
  await project.hasNot('.pnpm/node_modules/dep-of-pkg-with-1-dep')

  const modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies).toStrictEqual({ '/dep-of-pkg-with-1-dep/100.1.0': { dep: 'private' } })
})

test('should remove aliased hoisted dependencies', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['pkg-with-1-aliased-dep'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  await project.hasNot('pkg-with-1-aliased-dep')
  await project.hasNot('dep-of-pkg-with-1-dep')
  try {
    await resolveLinkTarget('./node_modules/dep')
    throw new Error('should have failed')
  } catch (e) {}

  const modules = await project.readModulesManifest()
  expect(modules).toBeTruthy()
  expect(modules!.hoistedDependencies).toStrictEqual({})
})

test('should update .modules.yaml when pruning if we are flattening', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'pkg-with-1-aliased-dep': '*',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  await install({}, await testDefaults({ hoistPattern: '*', pruneStore: true }))

  const modules = await project.readModulesManifest()
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
  }, await testDefaults({ fastUnpack: false, hoistPattern: '*' }))

  expect(project.requireModule('debug/package.json').version).toEqual('3.1.0')
  expect(project.requireModule('express/package.json').version).toEqual('4.16.0')

  await project.hasNot('.pnpm/node_modules/debug') // debug is not hoisted because it is a direct dep
  // read this module path because we can't use requireModule again, as it is cached
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that ms is still there, and that is-positive is not installed
  await install({
    dependencies: {
      express: '4.16.0',
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ fastUnpack: false, hoistPattern: '*', pruneStore: true }))

  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  expect(prevExpressModulePath).toEqual(currExpressModulePath)

  await project.has('.pnpm/node_modules/debug') // debug is hoisted because it is not a direct dep anymore
})

test('should hoist correctly peer dependencies', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ hoistPattern: '*' }))

  await project.has('.pnpm/node_modules/ajv-keywords')
})

test('should uninstall correctly peer dependencies', async () => {
  prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['using-ajv'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  // symlink to peer dependency is deleted
  expect(() => fs.lstatSync('node_modules/ajv-keywords')).toThrow()
})

test('hoist-pattern: hoist all dependencies to the virtual store node_modules', async () => {
  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',

    dependencies: {
      foobar: '100.0.0',
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
      buildIndex: 0,
      manifest: workspaceRootManifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      mutation: 'install',
      rootDir: path.resolve('package'),
    },
  ]
  await mutateModules(mutatedProjects, await testDefaults({ hoistPattern: '*' }))

  await projects['root'].has('pkg-with-1-dep')
  await projects['root'].has('.pnpm/node_modules/dep-of-pkg-with-1-dep')
  await projects['root'].has('.pnpm/node_modules/foobar')
  await projects['root'].has('.pnpm/node_modules/foo')
  await projects['root'].has('.pnpm/node_modules/bar')
  await projects['root'].hasNot('foobar')
  await projects['root'].hasNot('foo')
  await projects['root'].hasNot('bar')

  await projects['package'].has('foobar')
  await projects['package'].hasNot('foo')
  await projects['package'].hasNot('bar')

  await rimraf('node_modules')
  await rimraf('package/node_modules')

  await mutateModules(mutatedProjects, await testDefaults({ frozenLockfile: true, hoistPattern: '*' }))

  await projects['root'].has('pkg-with-1-dep')
  await projects['root'].has('.pnpm/node_modules/dep-of-pkg-with-1-dep')
  await projects['root'].has('.pnpm/node_modules/foobar')
  await projects['root'].has('.pnpm/node_modules/foo')
  await projects['root'].has('.pnpm/node_modules/bar')
  await projects['root'].hasNot('foobar')
  await projects['root'].hasNot('foo')
  await projects['root'].hasNot('bar')

  await projects['package'].has('foobar')
  await projects['package'].hasNot('foo')
  await projects['package'].hasNot('bar')
})

test('hoist when updating in one of the workspace projects', async () => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',

    dependencies: {
      foo: '100.0.0',
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
      buildIndex: 0,
      manifest: workspaceRootManifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      mutation: 'install',
      rootDir: path.resolve('package'),
    },
  ]
  await mutateModules(mutatedProjects, await testDefaults({ hoistPattern: '*' }))

  const rootModules = assertProject(process.cwd())
  {
    const modulesManifest = await rootModules.readModulesManifest()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({
      '/dep-of-pkg-with-1-dep/100.0.0': { 'dep-of-pkg-with-1-dep': 'private' },
      '/foo/100.0.0': { foo: 'private' },
    })
  }

  await mutateModules([
    {
      ...mutatedProjects[0],
      dependencySelectors: ['foo@100.1.0'],
      mutation: 'installSome',
    },
  ], await testDefaults({ hoistPattern: '*', pruneLockfileImporters: false }))

  const lockfile = await rootModules.readCurrentLockfile()

  expect(
    Object.keys(lockfile.packages)
  ).toStrictEqual(
    [
      '/dep-of-pkg-with-1-dep/100.0.0',
      '/foo/100.0.0',
      '/foo/100.1.0',
      '/pkg-with-1-dep/100.0.0',
    ]
  )

  {
    const modulesManifest = await rootModules.readModulesManifest()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({
      '/dep-of-pkg-with-1-dep/100.0.0': { 'dep-of-pkg-with-1-dep': 'private' },
    })
  }
})

test('should recreate node_modules with hoisting', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({ hoistPattern: undefined }))

  await project.hasNot('.pnpm/node_modules/dep-of-pkg-with-1-dep')
  {
    const modulesManifest = await project.readModulesManifest()
    expect(modulesManifest?.hoistPattern).toBeFalsy()
    expect(modulesManifest?.hoistedDependencies).toStrictEqual({})
  }

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  await project.has('pkg-with-1-dep')
  await project.has('.pnpm/node_modules/dep-of-pkg-with-1-dep')
  {
    const modulesManifest = await project.readModulesManifest()
    expect(modulesManifest?.hoistPattern).toBeTruthy()
    expect(Object.keys(modulesManifest?.hoistedDependencies ?? {}).length > 0).toBeTruthy()
  }
})

test('hoisting should not create a broken symlink to a skipped optional dependency', async () => {
  const project = prepareEmpty()
  console.log(process.cwd())

  await install({
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  }, await testDefaults({ publicHoistPattern: '*' }))

  await project.hasNot('dep-of-optional-pkg')

  // Verifying the same with headless installation
  await rimraf('node_modules')

  await install({
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  }, await testDefaults({ publicHoistPattern: '*' }))

  await project.hasNot('dep-of-optional-pkg')
})