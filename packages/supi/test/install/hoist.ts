import fs from 'fs'
import path from 'path'
import assertProject from '@pnpm/assert-project'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  MutatedProject,
  mutateModules,
} from 'supi'
import rimraf from '@zkochan/rimraf'
import resolveLinkTarget from 'resolve-link-target'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import { addDistTag, testDefaults } from '../utils'

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
  const rootModules = assertProject(process.cwd())

  const manifest = {
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  }

  await install(manifest, await testDefaults({ publicHoistPattern: '*' }))

  await project.hasNot('dep-of-optional-pkg')
  expect(await rootModules.readCurrentLockfile()).toStrictEqual(await rootModules.readLockfile())

  // Verifying the same with headless installation
  await rimraf('node_modules')

  await install(manifest, await testDefaults({ publicHoistPattern: '*' }))

  await project.hasNot('dep-of-optional-pkg')
  expect(await rootModules.readCurrentLockfile()).toStrictEqual(await rootModules.readLockfile())
})

test('the hoisted packages should not override the bin files of the direct dependencies', async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['hello-world-js-bin-parent'], await testDefaults({ fastUnpack: false, publicHoistPattern: '*' }))

  {
    const cmd = await fs.promises.readFile('node_modules/.bin/hello-world-js-bin', 'utf-8')
    expect(cmd).toContain('/hello-world-js-bin-parent/')
  }

  await rimraf('node_modules')

  await install(manifest, await testDefaults({ fastUnpack: false, frozenLockfile: true, publicHoistPattern: '*' }))

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
      buildIndex: 0,
      manifest: {
        name: 'root',
        version: '1.0.0',
      },
      mutation: 'install',
      rootDir: path.resolve('.'),
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
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]

  /**
   * project-1 locks is-positive@2.0.0 while project-2 locks is-positive@3.0.0
   * when partial install project@3.0.0, is-positive@3.0.0 always should be hoisted
   * instead of using is-positive@2.0.0 and does not hoist anything
   */
  await writeYamlFile(WANTED_LOCKFILE, {
    lockfileVersion: 5.3,
    importers: {
      '.': {
        specifiers: {},
      },
      'project-1': {
        specifiers: {
          'is-positive': '2.0.0',
        },
        dependencies: {
          'is-positive': '2.0.0',
        },
      },
      'project-2': {
        specifiers: {
          'is-positive': '3.0.0',
        },
        dependencies: {
          'is-positive': '3.0.0',
        },
      },
    },
    packages: {
      '/is-positive/2.0.0': {
        resolution: { integrity: 'sha1-sU8GvS24EK5sixJ0HRNr+u8Nh70=' },
        engines: { node: '>=0.10.0' },
        dev: false,
      },
      '/is-positive/3.0.0': {
        resolution: { integrity: 'sha1-jvDuIvfOJPdjP4kIAw7Ei2Ks9KM=' },
        engines: { node: '>=0.10.0' },
        dev: false,
      },
    },
  }, { lineWidth: 1000 })

  await mutateModules(importers, await testDefaults({ hoistPattern: '*' }))

  await root.has('.pnpm/node_modules/is-positive')
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

  const importers: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          '@babel/runtime-corejs3': '7.15.4',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]

  await writeYamlFile(WANTED_LOCKFILE, {
    lockfileVersion: 5.3,
    importers: {
      '.': {
        specifiers: {},
      },
      'project-1': {
        specifiers: {
          '@babel/runtime-corejs3': '7.15.3',
        },
        dependencies: {
          '@babel/runtime-corejs3': '7.15.3',
        },
      },
      'project-2': {
        specifiers: {
          '@babel/runtime-corejs3': '7.15.4',
        },
        dependencies: {
          '@babel/runtime-corejs3': '7.15.4',
        },
      },
    },
    packages: {
      '/@babel/runtime-corejs3/7.15.3': {
        resolution: { integrity: 'sha512-30A3lP+sRL6ml8uhoJSs+8jwpKzbw8CqBvDc1laeptxPm5FahumJxirigcbD2qTs71Sonvj1cyZB0OKGAmxQ+A==' },
        engines: { node: '>=6.9.0' },
        dependencies: {
          'core-js-pure': '3.17.2',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      '/@babel/runtime-corejs3/7.15.4': {
        resolution: { integrity: 'sha512-lWcAqKeB624/twtTc3w6w/2o9RqJPaNBhPGK6DKLSiwuVWC7WFkypWyNg+CpZoyJH0jVzv1uMtXZ/5/lQOLtCg==' },
        engines: { node: '>=6.9.0' },
        dependencies: {
          'core-js-pure': '3.17.3',
          'regenerator-runtime': '0.13.9',
        },
        dev: false,
      },
      '/core-js-pure/3.17.2': {
        resolution: { integrity: 'sha512-2VV7DlIbooyTI7Bh+yzOOWL9tGwLnQKHno7qATE+fqZzDKYr6llVjVQOzpD/QLZFgXDPb8T71pJokHEZHEYJhQ==' },
        requiresBuild: true,
        dev: false,
      },
      '/core-js-pure/3.17.3': {
        resolution: { integrity: 'sha512-YusrqwiOTTn8058JDa0cv9unbXdIiIgcgI9gXso0ey4WgkFLd3lYlV9rp9n7nDCsYxXsMDTjA4m1h3T348mdlQ==' },
        requiresBuild: true,
        dev: false,
      },
      '/regenerator-runtime/0.13.9': {
        resolution: { integrity: 'sha512-p3VT+cOEgxFsRRA9X4lkI1E+k2/CtnKtU4gcxyaCUreilL/vqI6CdZ3wxVUx3UOUg+gnUOQQcRI7BmSI656MYA==' },
        dev: false,
      },
    },
  }, { lineWidth: 1000 })

  await mutateModules(importers, await testDefaults({ hoistPattern: '*' }))

  await root.has('.pnpm/node_modules/@babel/runtime-corejs3')
  const { version: runtimeVersion } = root.requireModule('.pnpm/node_modules/@babel/runtime-corejs3/package.json')
  expect(runtimeVersion).toBe('7.15.4')

  await root.has('.pnpm/node_modules/core-js-pure')
  const { version: coreJsVersion } = root.requireModule('.pnpm/node_modules/core-js-pure/package.json')
  expect(coreJsVersion).toBe('3.17.3')

  await root.has('.pnpm/node_modules/regenerator-runtime')
  const { version: regeneratorVersion } = root.requireModule('.pnpm/node_modules/regenerator-runtime/package.json')
  expect(regeneratorVersion).toBe('0.13.9')
})
