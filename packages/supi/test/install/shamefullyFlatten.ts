import { prepareEmpty } from '@pnpm/prepare'
import fs = require('fs')
import resolveLinkTarget = require('resolve-link-target')
import { addDependenciesToPackage, install, mutateModules } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { addDistTag, testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('should flatten dependencies', async (t) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['express'], await testDefaults({ shamefullyFlatten: true }))

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')

  // should also flatten bins
  await project.isExecutable('.bin/mime')
})

test('should remove flattened dependencies', async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['express'], await testDefaults({ shamefullyFlatten: true }))
  await mutateModules([
    {
      dependencyNames: ['express'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ shamefullyFlatten: true }))

  await project.hasNot('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
})

test('should not override root packages with flattened dependencies', async (t) => {
  const project = prepareEmpty(t)

  // this installs debug@3.1.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0'], await testDefaults({ shamefullyFlatten: true }))
  // this installs express@4.16.2, that depends on debug 2.6.9, but we don't want to flatten debug@2.6.9
  await addDependenciesToPackage(manifest, ['express@4.16.2'], await testDefaults({ shamefullyFlatten: true }))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug did not get overridden by flattening')
})

test['skip']('should reflatten when uninstalling a package', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // this installs debug@3.1.0 and express@4.16.0
  const manifest = await addDependenciesToPackage({}, ['debug@3.1.0', 'express@4.16.0'], await testDefaults({ shamefullyFlatten: true }))
  // uninstall debug@3.1.0 to check if debug@2.6.9 gets reflattened
  await mutateModules([
    {
      dependencyNames: ['debug'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ shamefullyFlatten: true }))

  t.equal(project.requireModule('debug/package.json').version, '2.6.9', 'debug was flattened after uninstall')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express did not get updated by flattening')

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.importers['.'].hoistedAliases['localhost+4873/debug/2.6.9'], ['debug'], 'new hoisted debug added to .modules.yaml')
})

test('should reflatten after running a general install', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, await testDefaults({ shamefullyFlatten: true }))

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
  }, await testDefaults({ shamefullyFlatten: true }))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')
})

test('should not override aliased dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  // now I install is-negative, but aliased as "debug". I do not want the "debug" dependency of express to override my alias
  await addDependenciesToPackage({}, ['debug@npm:is-negative@1.0.0', 'express'], await testDefaults({ shamefullyFlatten: true }))

  t.equal(project.requireModule('debug/package.json').version, '1.0.0', 'alias respected by flattening')
})

test('--shamefully-flatten throws exception when executed on node_modules installed w/o the option', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ shamefullyFlatten: false }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ shamefullyFlatten: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err['code'], 'ERR_PNPM_SHAMEFULLY_FLATTEN_NOT_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created without the --shamefully-flatten option.') === 0)
  }
})

test('--no-shamefully-flatten throws exception when executed on node_modules installed with --shamefully-flatten', async (t: tape.Test) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ shamefullyFlatten: true }))

  try {
    await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ shamefullyFlatten: false }))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err['code'], 'ERR_PNPM_SHAMEFULLY_FLATTEN_WANTED') // tslint:disable-line:no-string-literal
    t.ok(err.message.indexOf('This "node_modules" folder was created using the --shamefully-flatten option.') === 0)
  }
})

test('flatten by alias', async (t: tape.Test) => {
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  const project = prepareEmpty(t)

  // pkg-with-1-aliased-dep aliases dep-of-pkg-with-1-dep as just "dep"
  await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ shamefullyFlatten: true }))

  await project.has('pkg-with-1-aliased-dep')
  await project.has('dep')
  await project.hasNot('dep-of-pkg-with-1-dep')

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.importers['.'].hoistedAliases, { 'localhost+4873/dep-of-pkg-with-1-dep/100.1.0': [ 'dep' ] }, '.modules.yaml updated correctly')
})

test('should remove aliased flattened dependencies', async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ shamefullyFlatten: true }))
  await mutateModules([
    {
      dependencyNames: ['pkg-with-1-aliased-dep'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ shamefullyFlatten: true }))

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
  t.deepEqual(modules!.importers['.'].hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should update .modules.yaml when pruning if we are flattening', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pkg-with-1-aliased-dep': '*',
    },
  }, await testDefaults({ shamefullyFlatten: true }))

  await install({}, await testDefaults({ shamefullyFlatten: true, pruneStore: true }))

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.deepEqual(modules!.importers['.'].hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should reflatten after pruning', async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0',
    },
  }, await testDefaults({ shamefullyFlatten: true }))

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
  }, await testDefaults({ shamefullyFlatten: true, pruneStore: true }))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')
})

test('should flatten correctly peer dependencies', async (t) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ shamefullyFlatten: true }))

  await project.has('ajv-keywords')
})

test('should uninstall correctly peer dependencies', async (t) => {
  prepareEmpty(t)
  const manifest = await addDependenciesToPackage({}, ['using-ajv'], await testDefaults({ shamefullyFlatten: true }))
  await mutateModules([
    {
      dependencyNames: ['using-ajv'],
      manifest,
      mutation: 'uninstallSome',
      prefix: process.cwd(),
    },
  ], await testDefaults({ shamefullyFlatten: true }))

  t.throws(() => fs.lstatSync('node_modules/ajv-keywords'), Error, 'symlink to peer dependency is deleted')
})
