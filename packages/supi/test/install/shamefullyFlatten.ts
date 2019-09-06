import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import fs = require('fs')
import path = require('path')
import resolveLinkTarget = require('resolve-link-target')
import {
  addDependenciesToPackage,
  install,
  MutatedImporter,
  mutateModules,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { addDistTag, testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('should hoist dependencies', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['express'], await testDefaults({ hoistPattern: '*' }))

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')
  await project.has('mime')

  // should also flatten bins
  await project.isExecutable('.bin/mime')
})

test('should hoist dependencies by pattern', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['express'], await testDefaults({ hoistPattern: 'mime' }))

  await project.has('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
  await project.has('mime')

  // should also flatten bins
  await project.isExecutable('.bin/mime')
})

test('should remove hoisted dependencies', async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['express'], await testDefaults({ hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['express'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  await project.hasNot('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
})

test('should not override root packages with hoisted dependencies', async (t) => {
  const project = prepareEmpty(t)

  // this installs debug@3.1.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0'], await testDefaults({ hoistPattern: '*' }))
  // this installs express@4.16.2, that depends on debug 2.6.9, but we don't want to flatten debug@2.6.9
  await addDependenciesToPackage(manifest, ['express@4.16.2'], await testDefaults({ hoistPattern: '*' }))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug did not get overridden by flattening')
})

test('should reflatten when uninstalling a package', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // this installs debug@3.1.0 and express@4.16.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0', 'express@4.16.0'], await testDefaults({ hoistPattern: '*' }))
  // uninstall debug@3.1.0 to check if debug@2.6.9 gets reflattened
  await mutateModules([
    {
      dependencyNames: ['debug'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  t.equal(project.requireModule('debug/package.json').version, '2.6.9', 'debug was flattened after uninstall')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express did not get updated by flattening')

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.hoistedAliases['localhost+4873/debug/2.6.9'], ['debug'], 'new hoisted debug added to .modules.yaml')
})

test('should rehoist after running a general install', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug installed correctly')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express installed correctly')

  // read this module path because we can't use requireModule again, as it is cached
  const prevDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that express stays at the same version
  await install({
    dependencies: {
      express: '4.16.0',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')
})

test('should not override aliased dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  // now I install is-negative, but aliased as "debug". I do not want the "debug" dependency of express to override my alias
  await addDependenciesToPackage({}, ['debug@npm:is-negative@1.0.0', 'express'], await testDefaults({ hoistPattern: '*' }))

  t.equal(project.requireModule('debug/package.json').version, '1.0.0', 'alias respected by flattening')
})

test('hoistPattern=* throws exception when executed on node_modules installed w/o the option', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ hoistPattern: undefined }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ hoistPattern: '*' }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_HOISTING_NOT_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created without the --hoist-pattern option.') === 0)
  }
})

test('hoistPattern=undefined throws exception when executed on node_modules installed with --shamefully-flatten', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ hoistPattern: '*' }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ hoistPattern: undefined }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err['code'], 'ERR_PNPM_HOISTING_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created using the --hoist-pattern option.') === 0)
  }
})

test('hoist by alias', async (t: tape.Test) => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  const project = prepareEmpty(t)

  // pkg-with-1-aliased-dep aliases dep-of-pkg-with-1-dep as just "dep"
  await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ hoistPattern: '*' }))

  await project.has('pkg-with-1-aliased-dep')
  await project.has('dep')
  await project.hasNot('dep-of-pkg-with-1-dep')

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.hoistedAliases, { 'localhost+4873/dep-of-pkg-with-1-dep/100.1.0': [ 'dep' ] }, '.modules.yaml updated correctly')
})

test('should remove aliased hoisted dependencies', async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['pkg-with-1-aliased-dep'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  await project.hasNot('pkg-with-1-aliased-dep')
  await project.hasNot('dep-of-pkg-with-1-dep')
  let caught = false
  try {
    await resolveLinkTarget('./node_modules/dep')
  } catch (e) {
    caught = true
  }
  t.ok(caught, 'dep removed correctly')

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should update .modules.yaml when pruning if we are flattening', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pkg-with-1-aliased-dep': '*',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  await install({}, await testDefaults({ hoistPattern: '*', pruneStore: true }))

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should reflatten after pruning', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug installed correctly')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express installed correctly')

  // read this module path because we can't use requireModule again, as it is cached
  const prevDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that ms is still there, and that is-positive is not installed
  await install({
    dependencies: {
      'express': '4.16.0',
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ hoistPattern: '*', pruneStore: true }))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')
})

test('should flatten correctly peer dependencies', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ hoistPattern: '*' }))

  await project.has('ajv-keywords')
})

test('should uninstall correctly peer dependencies', async (t) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ hoistPattern: '*' }))
  await mutateModules([
    {
      dependencyNames: ['using-ajv'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ hoistPattern: '*' }))

  t.throws(() => fs.lstatSync('node_modules/ajv-keywords'), Error, 'symlink to peer dependency is deleted')
})

test('shamefully-flatten: only hoists the dependencies of the root workspace package', async (t) => {
  const workspaceRootManifest = {
    name: 'root',

    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
  }
  const workspacePackageManifest = {
    name: 'package',

    dependencies: {
      'foobar': '100.0.0'
    },
  }
  const projects = preparePackages(t, [
    {
      location: '.',
      package: workspaceRootManifest,
    },
    {
      location: 'package',
      package: workspacePackageManifest,
    },
  ])

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      manifest: workspaceRootManifest,
      mutation: 'install',
      prefix: process.cwd(),
    },
    {
      buildIndex: 0,
      manifest: workspacePackageManifest,
      mutation: 'install',
      prefix: path.resolve('package'),
    },
  ]
  await mutateModules(importers, await testDefaults({ hoistPattern: '*' }))

  await projects['root'].has('pkg-with-1-dep')
  await projects['root'].has('dep-of-pkg-with-1-dep')
  await projects['root'].hasNot('foobar')
  await projects['root'].hasNot('foo')
  await projects['root'].hasNot('bar')

  await projects['package'].has('foobar')
  await projects['package'].hasNot('foo')
  await projects['package'].hasNot('bar')

  await rimraf('node_modules')
  await rimraf('package/node_modules')

  await mutateModules(importers, await testDefaults({ frozenLockfile: true, hoistPattern: '*' }))

  await projects['root'].has('pkg-with-1-dep')
  await projects['root'].has('dep-of-pkg-with-1-dep')
  await projects['root'].hasNot('foobar')
  await projects['root'].hasNot('foo')
  await projects['root'].hasNot('bar')

  await projects['package'].has('foobar')
  await projects['package'].hasNot('foo')
  await projects['package'].hasNot('bar')
})
