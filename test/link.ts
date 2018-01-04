import tape = require('tape')
import promisifyTape from 'tape-promise'
import sinon = require('sinon')
const test = promisifyTape(tape)
import path = require('path')
import writePkg = require('write-pkg')
import {
  prepare,
  isExecutable,
  pathToLocalPkg,
  testDefaults,
 } from './utils'
import thenify = require('thenify')
import ncpCB = require('ncp')
const ncp = thenify(ncpCB.ncp)
import {
  link,
  linkToGlobal,
  linkFromGlobal,
  installPkgs,
  RootLog,
} from 'supi'
import exists = require('path-exists')

test('relative link', async (t: tape.Test) => {
  prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link(`../${linkedPkgName}`, process.cwd(), await testDefaults())

  isExecutable(t, path.resolve('node_modules', '.bin', 'hello-world-js-bin'))

  // The linked package has been installed successfully as well with bins linked
  // to node_modules/.bin
  isExecutable(t, path.join(linkedPkgPath, 'node_modules', '.bin', 'cowsay'))
})

test('relative link is not rewritten by install', async (t: tape.Test) => {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link(`../${linkedPkgName}`, process.cwd(), await testDefaults())

  const reporter = sinon.spy()

  await installPkgs(['hello-world-js-bin'], await testDefaults({reporter}))

  t.ok(project.requireModule('hello-world-js-bin/package.json').isLocal)

  t.ok(reporter.calledWithMatch(<RootLog>{
    name: 'pnpm:root',
    level: 'debug',
    linked: {
      name: 'hello-world-js-bin',
      from: linkedPkgPath,
      to: path.resolve('node_modules'),
      // TODO: the dependencyType should be `undefined` in this case
    },
  }), 'linked root dependency logged')
})

test('global link', async function (t: tape.Test) {
  prepare(t)
  const projectPath = process.cwd()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  process.chdir(linkedPkgPath)
  const globalPrefix = path.resolve('..', 'global')
  const globalBin = path.resolve('..', 'global', 'bin')
  await linkToGlobal(process.cwd(), await testDefaults({globalPrefix, globalBin}))

  isExecutable(t, path.join(globalBin, 'hello-world-js-bin'))

  // bins of dependencies should not be linked, see issue https://github.com/pnpm/pnpm/issues/905
  t.notOk(await exists(path.join(globalBin, 'cowsay')), 'cowsay not linked')
  t.notOk(await exists(path.join(globalBin, 'cowthink')), 'cowthink not linked')

  process.chdir(projectPath)

  await linkFromGlobal(linkedPkgName, process.cwd(), await testDefaults({globalPrefix}))

  isExecutable(t, path.resolve('node_modules', '.bin', 'hello-world-js-bin'))
})

test('failed linking should not create empty folder', async (t: tape.Test) => {
  prepare(t)

  const globalPrefix = path.resolve('..', 'global')

  try {
    await linkFromGlobal('does-not-exist', process.cwd(), await testDefaults({globalPrefix}))
    t.fail('should have failed')
  } catch (err) {
    t.notOk(await exists(path.join(globalPrefix, 'node_modules', 'does-not-exist')))
  }
})
