import resolveLinkTarget = require('resolve-link-target')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {install, installPkgs, uninstall, prune} from 'supi'
import {prepare, testDefaults} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('should flatten dependencies', async function (t) {
  const project = prepare(t)

  await installPkgs(['express'], await testDefaults({shamefullyFlatten: true}))

  await project.has('express')
  await project.has('debug')
  await project.has('cookie')
})

test('should remove flattened dependencies', async function (t) {
  const project = prepare(t)

  await installPkgs(['express'], await testDefaults({shamefullyFlatten: true}))
  await uninstall(['express'], await testDefaults({shamefullyFlatten: true}))

  await project.hasNot('express')
  await project.hasNot('debug')
  await project.hasNot('cookie')
})

test('should not override root packages with flattened dependencies', async function (t) {
  const project = prepare(t)

  // this installs debug@3.1.0
  await installPkgs(['debug@3.1.0'], await testDefaults({shamefullyFlatten: true}))
  // this installs express@4.16.2, that depends on debug 2.6.9, but we don't want to flatten debug@2.6.9
  await installPkgs(['express@4.16.2'], await testDefaults({shamefullyFlatten: true}))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug did not get overridden by flattening')
})

test('should reflatten when uninstalling a package', async function (t) {
  const project = prepare(t)

  // this installs debug@3.1.0 and express@4.16.0
  await installPkgs(['debug@3.1.0', 'express@4.16.0'], await testDefaults({shamefullyFlatten: true}))
  // uninstall debug@3.1.0 to check if debug@2.6.9 gets reflattened
  await uninstall(['debug'], await testDefaults({shamefullyFlatten: true}))

  t.equal(project.requireModule('debug/package.json').version, '2.6.9', 'debug was flattened after uninstall')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express did not get updated by flattening')
})

test('should reflatten after running a general install', async function (t) {
  const project = prepare(t, {
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0'
    }
  })

  await install(await testDefaults({shamefullyFlatten: true}))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug installed correctly')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express installed correctly')

  // read this module path because we can't use requireModule again, as it is cached
  const prevDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that express stays at the same version
  await project.rewriteDependencies({
    express: '4.16.0'
  })

  await install(await testDefaults({shamefullyFlatten: true}))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')
})

test('should not override aliased dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  // now I install is-negative, but aliased as "debug". I do not want the "debug" dependency of express to override my alias
  await installPkgs(['debug@npm:is-negative@1.0.0', 'express'], await testDefaults({shamefullyFlatten: true}))

  t.equal(project.requireModule('debug/package.json').version, '1.0.0', 'alias respected by flattening')
})

test('--shamefully-flatten throws exception when executed on node_modules installed w/o the option', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({shamefullyFlatten: false}))

  try {
    await installPkgs(['is-negative'], await testDefaults({shamefullyFlatten: true}))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.indexOf('This node_modules was not installed with the --shamefully-flatten option.') === 0)
  }
})

test('--no-shamefully-flatten throws exception when executed on node_modules installed with --shamefully-flatten', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['is-positive'], await testDefaults({shamefullyFlatten: true}))

  try {
    await installPkgs(['is-negative'], await testDefaults({shamefullyFlatten: false}))
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.indexOf('This node_modules was installed with --shamefully-flatten option.') === 0)
  }
})

test('flatten by alias', async (t: tape.Test) => {
  const project = prepare(t)

  // pkg-with-1-aliased-dep aliases dep-of-pkg-with-1-dep as just "dep"
  await installPkgs(['pkg-with-1-aliased-dep'], await testDefaults({shamefullyFlatten: true}))

  await project.has('pkg-with-1-aliased-dep')
  await project.has('dep')
  await project.hasNot('dep-of-pkg-with-1-dep')

  const modules = await project.loadModules()
  t.deepEqual(modules.hoistedAliases, {'localhost+4873/dep-of-pkg-with-1-dep/100.1.0': [ 'dep' ]}, '.modules.yaml updated correctly')
})

test('should remove aliased flattened dependencies', async function (t) {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-aliased-dep'], await testDefaults({shamefullyFlatten: true}))
  await uninstall(['pkg-with-1-aliased-dep'], await testDefaults({shamefullyFlatten: true}))

  await project.hasNot('pkg-with-1-aliased-dep')
  await project.hasNot('dep-of-pkg-with-1-dep')
  let caught = false
  try {
    await resolveLinkTarget('./node_modules/dep')
  } catch (e) {
    caught = true
  }
  t.ok(caught, 'dep removed correctly')

  const modules = await project.loadModules()
  t.deepEqual(modules.hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should update .modules.yaml when pruning if we are flattening', async function (t) {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-1-aliased-dep': '*'
    }
  })

  await install(await testDefaults({shamefullyFlatten: true}))

  await project.rewriteDependencies({})

  await prune(await testDefaults({shamefullyFlatten: true}))

  const modules = await project.loadModules()
  t.deepEqual(modules.hoistedAliases, {}, '.modules.yaml updated correctly')
})

test('should reflatten after pruning', async function (t) {
  const project = prepare(t, {
    dependencies: {
      debug: '3.1.0',
      express: '4.16.0'
    }
  })

  await install(await testDefaults({shamefullyFlatten: true}))

  t.equal(project.requireModule('debug/package.json').version, '3.1.0', 'debug installed correctly')
  t.equal(project.requireModule('express/package.json').version, '4.16.0', 'express installed correctly')

  // read this module path because we can't use requireModule again, as it is cached
  const prevDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const prevExpressModulePath = await resolveLinkTarget('./node_modules/express')

  // now remove debug@3.1.0 from package.json, run install again, check that debug@2.6.9 has been flattened
  // and that ms is still there, and that is-positive is not installed
  await project.rewriteDependencies({
    express: '4.16.0',
    'is-positive': '1.0.0',
  })

  await prune(await testDefaults({shamefullyFlatten: true}))

  const currDebugModulePath = await resolveLinkTarget('./node_modules/debug')
  const currExpressModulePath = await resolveLinkTarget('./node_modules/express')
  t.notEqual(prevDebugModulePath, currDebugModulePath, 'debug flattened correctly')
  t.equal(prevExpressModulePath, currExpressModulePath, 'express not updated')

  await project.hasNot('is-positive')
})

test('should flatten correctly peer dependencies', async function (t) {
  const project = prepare(t)
  await installPkgs(['using-ajv'], await testDefaults({shamefullyFlatten: true}))

  await project.has('ajv-keywords')
})
