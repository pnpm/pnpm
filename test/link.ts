import tape = require('tape')
import promisifyTape = require('tape-promise')
const test = promisifyTape(tape)
import path = require('path')
import isExecutable from './support/isExecutable'
import prepare from './support/prepare'
import mkdirp = require('mkdirp')
import thenify = require('thenify')
import ncpCB = require('ncp')
const ncp = thenify(ncpCB.ncp)
import link from '../src/cmd/link'
import install from '../src/cmd/install'
import globalPath from './support/globalPath'
import {pathToLocalPkg} from './support/localPkg'

test('relative link', async function (t) {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link([`../${linkedPkgDirName}`], { quiet: true })

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('global link', async function (t) {
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  mkdirp.sync(tmpDir)
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgName + Math.random().toString())

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  process.chdir(linkedPkgPath)
  await link([], { globalPath, quiet: true })

  prepare()
  await link([linkedPkgName], { globalPath, quiet: true })

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})

test('link local package if link-local = true', async function (t) {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await install([`file:../${linkedPkgDirName}`], { quiet: true, linkLocal: true })

  isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))
})
