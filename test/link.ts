import tape = require('tape')
import promisifyTape from 'tape-promise'
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
  cmd,
} from '../src'

test('relative link', async function (t) {
  prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link(`../${linkedPkgName}`, process.cwd(), testDefaults())

  isExecutable(t, path.resolve('node_modules', '.bin', 'hello-world-js-bin'))
})

test('relative link is not rewritten by install', async function (t) {
  const project = prepare(t)

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
  await link(`../${linkedPkgName}`, process.cwd(), testDefaults())

  await installPkgs(['hello-world-js-bin'], testDefaults())

  t.ok(project.requireModule('hello-world-js-bin/package.json').isLocal)
})

test('global link', async function (t) {
  prepare(t)
  const projectPath = process.cwd()

  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve('..', linkedPkgName)

  await ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)

  process.chdir(linkedPkgPath)
  const globalPrefix = path.resolve('..', 'global')
  await linkToGlobal(process.cwd(), Object.assign(testDefaults(), {globalPrefix}))

  process.chdir(projectPath)

  await linkFromGlobal(linkedPkgName, process.cwd(), Object.assign(testDefaults(), {globalPrefix}))

  isExecutable(t, path.resolve('node_modules', '.bin', 'hello-world-js-bin'))
})

test('linking multiple packages', async (t: tape.Test) => {
  const project = prepare(t)

  process.chdir('..')
  const globalPrefix = path.resolve('global')

  await writePkg('linked-foo', {name: 'linked-foo', version: '1.0.0'})
  await writePkg('linked-bar', {name: 'linked-bar', version: '1.0.0'})

  process.chdir('linked-foo')

  const opts = Object.assign(testDefaults(), {globalPrefix})

  t.comment('linking linked-foo to global package')
  await cmd.link([], opts)

  process.chdir('..')
  process.chdir('project')

  await cmd.link(['linked-foo', '../linked-bar'], opts)

  project.has('linked-foo')
  project.has('linked-bar')
})
