import tape = require('tape')
import promisifyTape from 'tape-promise'
const test = promisifyTape(tape)
import path = require('path')
import isExecutable from './support/isExecutable'
import prepare from './support/prepare'
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
import {pathToLocalPkg} from './support/localPkg'
import testDefaults from './support/testDefaults'
import globalPath from './support/globalPath'

test('relative link', async function (t) {
  prepare(t)
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await linkFromRelative(`../${linkedPkgDirName}`, testDefaults())

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('global link', async function (t) {
  // NOTE: the linked packages should use the same store.
  // Otherwise it would be a mess the linked package could use a flat dependency tree
  // while the parent package could use a nested one
  const storePath = path.join(globalPath, '.store')

  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  mkdirp.sync(tmpDir)
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgName + Math.random().toString())

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  process.chdir(linkedPkgPath)
  await linkToGlobal(testDefaults({storePath}))

  prepare(t)
  await linkFromGlobal(linkedPkgName, testDefaults({storePath}))

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('link local package if link-local = true', async function (t) {
  prepare(t)
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await installPkgs([`file:../${linkedPkgDirName}`], testDefaults({ linkLocal: true }))

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})
