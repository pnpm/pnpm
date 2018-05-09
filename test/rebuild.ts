import exists = require('path-exists')
import {
  installPkgs,
  rebuild,
  rebuildPkgs,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  pathToLocalPkg,
  prepare,
  testDefaults,
 } from './utils'

const test = promisifyTape(tape)

test('rebuilds dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example#prepare'], await testDefaults({saveDev: true, ignoreScripts: true}))

  let modules = await project.loadModules()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb',
  ])

  await rebuild(await testDefaults())

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js'))
    t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
    t.equal(scripts[0], 'prepare')
    t.equal(scripts[1], 'preinstall')
    t.equal(scripts[2], 'install')
    t.equal(scripts[3], 'postinstall')
  }
})

test('rebuilds specific dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example'], await testDefaults({saveDev: true, ignoreScripts: true}))

  await rebuildPkgs(['install-scripts-example-for-pnpm'], await testDefaults())

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('rebuild with pending option', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], await testDefaults({ignoreScripts: true}))
  await installPkgs(['zkochan/install-scripts-example'], await testDefaults({ignoreScripts: true}))

  let modules = await project.loadModules()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/6d879afcee10ece4d3f0e8c09de2993232f3430a',
  ])

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  await project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  await project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild(await testDefaults({rawNpmConfig: {pending: true}}))

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

test('rebuild dependencies in correct order', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['with-postinstall-a'], await testDefaults({ignoreScripts: true}))

  let modules = await project.loadModules()
  t.ok(modules)
  t.doesNotEqual(modules!.pendingBuilds.length, 0)

  await project.hasNot('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')
  await project.hasNot('with-postinstall-a/output.json')

  await rebuild(await testDefaults({rawNpmConfig: {pending: true}}))

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  t.ok(+project.requireModule('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
})
