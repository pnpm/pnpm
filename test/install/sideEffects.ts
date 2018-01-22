import fs = require('mz/fs')

import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  execPnpm,
} from '../utils'
import path = require('path')
import loadJsonFile = require('load-json-file')
import rimraf = require('rimraf-then')

const pkgRoot = path.join(__dirname, '..', '..')
const pnpmPkg = loadJsonFile.sync(path.join(pkgRoot, 'package.json'))

const test = promisifyTape(tape)

test.only('caching side effects of native package', async function (t) {
  const project = prepare(t)

  await execPnpm('install', '--side-effects-cache', 'runas@3.1.1')
  const storePath = await project.getStorePath()
  const cacheBuildDir = path.join(storePath, 'localhost+4873', 'runas', '3.1.1', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')
  const stat1 = await fs.stat(cacheBuildDir)

  t.ok(await fs.exists(path.join('node_modules', 'runas', 'build')), 'build folder created')
  t.ok(await fs.exists(cacheBuildDir), 'build folder created in side effects cache')

  await execPnpm('install', 'runas@3.1.1', '--side-effects-cache')
  const stat2 = await fs.stat(cacheBuildDir)
  t.equal(stat1.ino, stat2.ino, 'existing cache is not overridden')

  await execPnpm('install', 'runas@3.1.1', '--side-effects-cache', '--force')
  const stat3 = await fs.stat(cacheBuildDir)
  t.notEqual(stat1.ino, stat3.ino, 'cache is overridden when force is true')

  t.end()
})

test('using side effects cache', async function (t) {
  const project = prepare(t)

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  await execPnpm(...'install runas@3.1.1 --side-effects-cache --no-verify-store-integrity --package-import-method copy'.split(' '))
  const storePath = await project.getStorePath()

  const cacheBuildDir = path.join(storePath, 'localhost+4873', 'runas', '3.1.1', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(...'install runas@3.1.1 --side-effects-cache --no-verify-store-integrity --package-import-method copy'.split(' '))

  t.ok(await fs.exists(path.join('node_modules', 'runas', 'build', 'new-file.txt')), 'side effects cache correctly used')

  t.end()
})

test('readonly side effects cache', async function (t) {
  const project = prepare(t)

  await execPnpm(...'install runas@3.1.1 --side-effects-cache --no-verify-store-integrity'.split(' '))
  const storePath = await project.getStorePath()

  // Modify the side effects cache to make sure we are using it
  const cacheBuildDir = path.join(storePath, 'localhost+4873', 'runas', '3.1.1', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(...'install runas@3.1.1 --side-effects-cache-readonly --no-verify-store-integrity --package-import-method copy'.split(' '))

  t.ok(await fs.exists(path.join('node_modules', 'runas', 'build', 'new-file.txt')), 'readonly side effects cache correctly used')

  await rimraf('node_modules')
  // changing version to make sure we don't create the cache
  await execPnpm(...'install runas@3.1.0 --side-effects-cache-readonly --no-verify-store-integrity --package-import-method copy'.split(' '))

  t.ok(await fs.exists(path.join('node_modules', 'runas', 'build')), 'build folder created')
  t.notOk(await fs.exists(path.join(storePath, 'localhost+4873', 'runas', '3.1.0', 'side_effects', `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`, 'package', 'build')), 'cache folder not created')

  t.end()
})
