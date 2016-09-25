import test = require('tape')
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

test('relative link', t => {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => link([`../${linkedPkgDirName}`], { quiet: true }))
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})

test('global link', t => {
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  mkdirp.sync(tmpDir)
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgName + Math.random().toString())
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => {
      process.chdir(linkedPkgPath)
      return link([], { globalPath, quiet: true })
    })
    .then(() => {
      prepare()
      return link([linkedPkgName], { globalPath, quiet: true })
    })
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})

test('link local package if link-local = true', t => {
  prepare()
  const tmpDir = path.resolve(__dirname, '..', '.tmp')
  const linkedPkgName = 'hello-world-js-bin'
  const linkedPkgDirName = linkedPkgName + Math.random().toString()
  const linkedPkgPath = path.resolve(tmpDir, linkedPkgDirName)
  ncp(pathToLocalPkg(linkedPkgName), linkedPkgPath)
    .then(() => install([`file:../${linkedPkgDirName}`], { quiet: true, linkLocal: true }))
    .then(() => {
      isExecutable(t, path.join(process.cwd(), 'node_modules', '.bin', 'hello-world-js-bin'))

      t.end()
    })
    .catch(t.end)
})