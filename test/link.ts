import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import {
  prepare,
  isExecutable,
  pathToLocalPkg,
  testDefaults,
 } from './utils'
import mkdirp = require('mkdirp')
import thenify = require('thenify')
import ncpCB = require('ncp')
const ncp = thenify(ncpCB.ncp)
import {
  linkFromRelative,
  linkToGlobal,
  linkFromGlobal,
  installPkgs
} from '../src'

test('relative link', async function (t) {
  prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(process.cwd(), '..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await linkFromRelative(`../${linkedPkgName}`, testDefaults())

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('global link', async function (t) {
  prepare(t)
  const projectPath = process.cwd()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(process.cwd(), '..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  process.chdir(linkedPkgPath)
  await linkToGlobal(testDefaults())

  process.chdir(projectPath)

  await linkFromGlobal(linkedPkgName, testDefaults())

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('link local package if link-local = true', async function (t) {
  prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(process.cwd(), '..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await installPkgs([`file:../${linkedPkgName}`], testDefaults({ linkLocal: true }))

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})
