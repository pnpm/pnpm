import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import tape = require('tape')

const test = promisifyTape(tape)

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.skip('caching side effects of native package', async function (t) {
  const project = prepare(t)

  await execPnpm(['add', '--side-effects-cache', 'diskusage@1.1.3'])
  const storePath = await project.getStorePath()
  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = await fs.stat(cacheBuildDir)

  t.ok(await fs.exists(path.join('node_modules/diskusage/build')), 'build folder created')
  t.ok(await fs.exists(cacheBuildDir), 'build folder created in side effects cache')

  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache'])
  const stat2 = await fs.stat(cacheBuildDir)
  t.equal(stat1.ino, stat2.ino, 'existing cache is not overridden')

  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--force'])
  const stat3 = await fs.stat(cacheBuildDir)
  t.notEqual(stat1.ino, stat3.ino, 'cache is overridden when force is true')

  t.end()
})

test.skip('using side effects cache', async function (t) {
  const project = prepare(t)

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])
  const storePath = await project.getStorePath()

  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  t.ok(await fs.exists('node_modules/diskusage/build/new-file.txt'), 'side effects cache correctly used')

  t.end()
})

test.skip('readonly side effects cache', async function (t) {
  const project = prepare(t)

  await execPnpm(['add', 'diskusage@1.1.2', '--side-effects-cache', '--no-verify-store-integrity'])
  const storePath = await project.getStorePath()

  // Modify the side effects cache to make sure we are using it
  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.2/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(['add', 'diskusage@1.1.2', '--side-effects-cache-readonly', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  t.ok(await fs.exists('node_modules/diskusage/build/new-file.txt'), 'readonly side effects cache correctly used')

  await rimraf('node_modules')
  // changing version to make sure we don't create the cache
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache-readonly', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  t.ok(await fs.exists('node_modules/diskusage/build'), 'build folder created')
  t.notOk(await fs.exists(path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)), 'cache folder not created')

  t.end()
})
