import { promises as fs, readFileSync } from 'fs'
import path from 'path'
import { addDependenciesToPackage } from '@pnpm/core'
import { getFilePathInCafs, PackageFilesIndex } from '@pnpm/cafs'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { prepareEmpty } from '@pnpm/prepare'
import { ENGINE_NAME } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import exists from 'path-exists'
import writeJsonFile from 'write-json-file'
import { testDefaults } from '../utils'

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.skip('caching side effects of native package', async () => {
  prepareEmpty()

  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  let manifest = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts)
  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = await fs.stat(cacheBuildDir)

  expect(await exists('node_modules/diskusage/build')).toBeTruthy()
  expect(await exists(cacheBuildDir)).toBeTruthy()

  manifest = await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)
  const stat2 = await fs.stat(cacheBuildDir)
  expect(stat1.ino).toBe(stat2.ino)

  opts.force = true
  await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)
  const stat3 = await fs.stat(cacheBuildDir)

  // cache is overridden when force is true
  expect(stat1.ino).not.toBe(stat3.ino)
})

test.skip('caching side effects of native package when hoisting is used', async () => {
  const project = prepareEmpty()

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
  expect(await exists(cacheBuildDir)).toBeTruthy() // build folder created in side effects cache
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat2 = await fs.stat(cacheBuildDir)
  expect(stat1.ino).toBe(stat2.ino) // existing cache is not overridden
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  opts.force = true
  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat3 = await fs.stat(cacheBuildDir)
  expect(stat1.ino).not.toBe(stat3.ino) // cache is overridden when force is true
  await project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created
})

test('using side effects cache', async () => {
  prepareEmpty()

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts)

  const cafsDir = path.join(opts.storeDir, 'files')
  const filesIndexFile = getFilePathInCafs(cafsDir, getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), 'index')
  const filesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
  expect(filesIndex.sideEffects).toBeTruthy() // files index has side effects
  const sideEffectsKey = `${ENGINE_NAME}-${JSON.stringify({ '/@pnpm.e2e/hello-world-js-bin/1.0.0': {} })}`
  expect(filesIndex.sideEffects).toHaveProperty([sideEffectsKey, 'generated-by-preinstall.js'])
  expect(filesIndex.sideEffects).toHaveProperty([sideEffectsKey, 'generated-by-postinstall.js'])
  delete filesIndex.sideEffects![sideEffectsKey]['generated-by-postinstall.js']
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
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts2)

  expect(await exists(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy() // side effects cache correctly used
  expect(await exists(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeFalsy() // side effects cache correctly used
})

test.skip('readonly side effects cache', async () => {
  prepareEmpty()

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

  expect(await exists('node_modules/diskusage/build/new-file.txt')).toBeTruthy()

  await rimraf('node_modules')
  // changing version to make sure we don't create the cache
  await addDependenciesToPackage(manifest, ['diskusage@1.1.2'], opts2)

  expect(await exists('node_modules/diskusage/build')).toBeTruthy()
  expect(await exists(path.join(opts2.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.2/side_effects/${ENGINE_DIR}/package/build`))).toBeFalsy()
})

test('uploading errors do not interrupt installation', async () => {
  prepareEmpty()

  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  opts.storeController.upload = async () => {
    throw new Error('an unexpected error')
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts)

  expect(await exists('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()

  const cafsDir = path.join(opts.storeDir, 'files')
  const filesIndexFile = getFilePathInCafs(cafsDir, getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), 'index')
  const filesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
  expect(filesIndex.sideEffects).toBeFalsy()
})

test('a postinstall script does not modify the original sources added to the store', async () => {
  prepareEmpty()

  const opts = await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await addDependenciesToPackage({}, ['@pnpm/postinstall-modifies-source@1.0.0'], opts)

  expect(readFileSync('node_modules/@pnpm/postinstall-modifies-source/empty-file.txt', 'utf8')).toContain('hello')

  const cafsDir = path.join(opts.storeDir, 'files')
  const filesIndexFile = getFilePathInCafs(cafsDir, getIntegrity('@pnpm/postinstall-modifies-source', '1.0.0'), 'index')
  const filesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
  const patchedFileIntegrity = filesIndex.sideEffects?.[`${ENGINE_NAME}-{}`]['empty-file.txt']?.integrity
  expect(patchedFileIntegrity).toBeTruthy()
  const originalFileIntegrity = filesIndex.files['empty-file.txt'].integrity
  expect(originalFileIntegrity).toBeTruthy()
  // The integrity of the original file differs from the integrity of the patched file
  expect(originalFileIntegrity).not.toEqual(patchedFileIntegrity)

  expect(readFileSync(getFilePathInCafs(cafsDir, originalFileIntegrity, 'nonexec'), 'utf8')).toEqual('')
})
