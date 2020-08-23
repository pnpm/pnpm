import promisifyTape from 'tape-promise'
import { ENGINE_NAME } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from 'supi'
import { PackageFilesIndex } from '@pnpm/cafs'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import exists = require('path-exists')
import tape = require('tape')
import writeJsonFile = require('write-json-file')

const test = promisifyTape(tape)

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.skip('caching side effects of native package', async (t) => {
  prepareEmpty(t)

  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  let manifest = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts)
  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = await fs.stat(cacheBuildDir)

  t.ok(await exists('node_modules/diskusage/build'), 'build folder created')
  t.ok(await exists(cacheBuildDir), 'build folder created in side effects cache')

  manifest = await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)
  const stat2 = await fs.stat(cacheBuildDir)
  t.equal(stat1.ino, stat2.ino, 'existing cache is not overridden')

  opts.force = true
  await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)
  const stat3 = await fs.stat(cacheBuildDir)
  t.notEqual(stat1.ino, stat3.ino, 'cache is overridden when force is true')
})

test.skip('caching side effects of native package when hoisting is used', async (t) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults({
    fastUnpack: false,
    hoistPattern: '*',
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  const manifest = await addDependenciesToPackage({}, ['expire-fs@2.2.3'], opts)
  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = await fs.stat(cacheBuildDir)

  await project.has('.pnpm/node_modules/diskusage/build') // build folder created
  t.ok(await exists(cacheBuildDir), 'build folder created in side effects cache')
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat2 = await fs.stat(cacheBuildDir)
  t.equal(stat1.ino, stat2.ino, 'existing cache is not overridden')
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  opts.force = true
  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat3 = await fs.stat(cacheBuildDir)
  t.notEqual(stat1.ino, stat3.ino, 'cache is overridden when force is true')
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created
})

test('using side effects cache', async (t) => {
  prepareEmpty(t)

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  const manifest = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts)

  const filesIndexFile = path.join(opts.storeDir, 'files/10/0c9ac65f21cb83e1d3b9339731937e96d930d0000075d266d3443307659d27759e81f3bc0e87b202ade1f10c4af6845d060b4a985ee6b3ccc4de163a3d2171-index.json')
  const filesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
  t.ok(filesIndex.sideEffects, 'files index has side effects')
  t.ok(filesIndex.sideEffects[ENGINE_NAME]['build/Makefile'])
  delete filesIndex.sideEffects[ENGINE_NAME]['build/Makefile']
  await writeJsonFile(filesIndexFile, filesIndex)

  await rimraf('node_modules')
  await rimraf('pnpm-lock.yaml') // to avoid headless install
  const opts2 = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts2)

  t.notOk(await exists(path.resolve('node_modules/diskusage/build/Makefile')), 'side effects cache correctly used')
  t.ok(await exists(path.resolve('node_modules/diskusage/build/binding.Makefile')), 'side effects cache correctly used')
})

test.skip('readonly side effects cache', async (t) => {
  prepareEmpty(t)

  const opts1 = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  })
  let manifest = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts1)

  // Modify the side effects cache to make sure we are using it
  const cacheBuildDir = path.join(opts1.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  const opts2 = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: false,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  manifest = await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts2)

  t.ok(await exists('node_modules/diskusage/build/new-file.txt'), 'side effects cache correctly used')

  await rimraf('node_modules')
  // changing version to make sure we don't create the cache
  await addDependenciesToPackage(manifest, ['diskusage@1.1.2'], opts2)

  t.ok(await exists('node_modules/diskusage/build'), 'build folder created')
  t.notOk(await exists(path.join(opts2.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.2/side_effects/${ENGINE_DIR}/package/build`)), 'cache folder not created')
})

test('uploading errors do not interrupt installation', async (t) => {
  prepareEmpty(t)

  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  opts.storeController.upload = async () => {
    throw new Error('an unexpected error')
  }
  await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts)

  t.ok(await exists('node_modules/diskusage/build'), 'build folder created')

  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  t.notOk(await exists(cacheBuildDir), 'side effects cache not created')

  t.end()
})
