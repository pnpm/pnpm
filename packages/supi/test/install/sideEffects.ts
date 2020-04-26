import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import { addDependenciesToPackage } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

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
  let manifest = await addDependenciesToPackage({}, ['expire-fs@2.2.3'], opts)
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

test.skip('using side effects cache', async (t) => {
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

  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)

  t.ok(await exists('node_modules/diskusage/build/new-file.txt'), 'side effects cache correctly used')
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

test.skip('uploading errors do not interrupt installation', async (t) => {
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
