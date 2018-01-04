import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  isExecutable,
  pathToLocalPkg,
  testDefaults,
 } from './utils'
import {
  rebuild,
  rebuildPkgs,
  installPkgs,
} from 'supi'

const test = promisifyTape(tape)

test('rebuilds dependencies', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example'], await testDefaults({saveDev: true, ignoreScripts: true}))

  await rebuild(await testDefaults())

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

test('rebuilds specific dependencies', async function (t: tape.Test) {
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

test('rebuild with pending option', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['pre-and-postinstall-scripts-example'], await testDefaults({ignoreScripts: true}))
  await installPkgs(['zkochan/install-scripts-example'], await testDefaults({ignoreScripts: true}))

  let modules = await project.loadModules()
  t.doesNotEqual(modules['pendingBuilds'].length, 0)

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  await project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  await project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild(await testDefaults({rawNpmConfig: {'pending': true}}))

  modules = await project.loadModules()
  t.equal(modules['pendingBuilds'].length, 0)

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
