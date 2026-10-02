import fs from 'node:fs'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getFilePathByModeInCafs, isSymlinkMode, type PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { rimrafSync } from '@zkochan/rimraf'

import { testDefaults } from '../utils/index.js'

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

test.skip('caching side effects of native package', async () => {
  prepareEmpty()

  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  let { updatedManifest: manifest } = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts)
  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = fs.statSync(cacheBuildDir)

  expect(fs.existsSync('node_modules/diskusage/build')).toBeTruthy()
  expect(fs.existsSync(cacheBuildDir)).toBeTruthy()

  manifest = (await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)).updatedManifest
  const stat2 = fs.statSync(cacheBuildDir)
  expect(stat1.ino).toBe(stat2.ino)

  opts.force = true
  await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts)
  const stat3 = fs.statSync(cacheBuildDir)

  // cache is overridden when force is true
  expect(stat1.ino).not.toBe(stat3.ino)
})

test.skip('caching side effects of native package when hoisting is used', async () => {
  const project = prepareEmpty()

  const opts = testDefaults({
    fastUnpack: false,
    hoistPattern: '*',
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['expire-fs@2.2.3'], opts)
  const cacheBuildDir = path.join(opts.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = fs.statSync(cacheBuildDir)

  project.has('.pnpm/node_modules/diskusage/build') // build folder created
  expect(fs.existsSync(cacheBuildDir)).toBeTruthy() // build folder created in side effects cache
  project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat2 = fs.statSync(cacheBuildDir)
  expect(stat1.ino).toBe(stat2.ino) // existing cache is not overridden
  project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created

  opts.force = true
  await addDependenciesToPackage(manifest, ['expire-fs@2.2.3'], opts)
  const stat3 = fs.statSync(cacheBuildDir)
  expect(stat1.ino).not.toBe(stat3.ino) // cache is overridden when force is true
  project.has('.pnpm/node_modules/es6-promise') // verifying that a flat node_modules was created
})

test('using side effects cache', async () => {
  prepareEmpty()

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }, {}, {}, { packageImportMethod: 'copy' })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts)

  const filesIndexKey = storeIndexKey(getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeTruthy() // files index has side effects
  const sideEffectsKey = `${ENGINE_NAME};format=2;deps=${hashObject({
    id: `@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0:${getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')}`,
    deps: {
      '@pnpm.e2e/hello-world-js-bin': hashObject({
        id: `@pnpm.e2e/hello-world-js-bin@1.0.0:${getIntegrity('@pnpm.e2e/hello-world-js-bin', '1.0.0')}`,
        deps: {},
      }),
    },
  })}`
  expect(filesIndex.sideEffects).toBeTruthy()
  expect(filesIndex.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  const addedFiles = filesIndex.sideEffects!.get(sideEffectsKey)!.added!
  expect(addedFiles.has('generated-by-preinstall.js')).toBeTruthy()
  expect(addedFiles.has('generated-by-postinstall.js')).toBeTruthy()
  addedFiles.delete('generated-by-postinstall.js')
  storeIndex.set(filesIndexKey, filesIndex)

  rimrafSync('node_modules')
  rimrafSync('pnpm-lock.yaml') // to avoid headless install
  const opts2 = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    storeDir: opts.storeDir,
    verifyStoreIntegrity: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }, {}, {}, { packageImportMethod: 'copy' })
  await addDependenciesToPackage(manifest, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts2)

  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy() // side effects cache correctly used
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeFalsy() // side effects cache correctly used
})

test.skip('readonly side effects cache', async () => {
  prepareEmpty()

  const opts1 = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
  })
  let { updatedManifest: manifest } = await addDependenciesToPackage({}, ['diskusage@1.1.3'], opts1)

  // Modify the side effects cache to make sure we are using it
  const cacheBuildDir = path.join(opts1.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  fs.writeFileSync(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  rimrafSync('node_modules')
  const opts2 = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: false,
    verifyStoreIntegrity: false,
  }, {}, {}, { packageImportMethod: 'copy' })
  manifest = (await addDependenciesToPackage(manifest, ['diskusage@1.1.3'], opts2)).updatedManifest

  expect(fs.existsSync('node_modules/diskusage/build/new-file.txt')).toBeTruthy()

  rimrafSync('node_modules')
  // changing version to make sure we don't create the cache
  await addDependenciesToPackage(manifest, ['diskusage@1.1.2'], opts2)

  expect(fs.existsSync('node_modules/diskusage/build')).toBeTruthy()
  expect(fs.existsSync(path.join(opts2.storeDir, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.2/side_effects/${ENGINE_DIR}/package/build`))).toBeFalsy()
})

test('uploading errors do not interrupt installation', async () => {
  prepareEmpty()

  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  })
  opts.storeController.upload = async () => {
    throw new Error('an unexpected error')
  }
  await addDependenciesToPackage({}, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts)

  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()

  const filesIndexKey2 = storeIndexKey(getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const storeIndex2 = new StoreIndex(opts.storeDir)
  const filesIndex2 = storeIndex2.get(filesIndexKey2) as PackageFilesIndex
  storeIndex2.close()
  expect(filesIndex2.sideEffects).toBeFalsy()
})

test('a postinstall script does not modify the original sources added to the store', async () => {
  prepareEmpty()

  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    allowBuilds: { '@pnpm/postinstall-modifies-source': true },
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await addDependenciesToPackage({}, ['@pnpm/postinstall-modifies-source@1.0.0'], opts)

  expect(fs.readFileSync('node_modules/@pnpm/postinstall-modifies-source/empty-file.txt', 'utf8')).toContain('hello')

  const filesIndexKey3 = storeIndexKey(getIntegrity('@pnpm/postinstall-modifies-source', '1.0.0'), '@pnpm/postinstall-modifies-source@1.0.0')
  const storeIndex3 = new StoreIndex(opts.storeDir)
  const filesIndex = storeIndex3.get(filesIndexKey3) as PackageFilesIndex
  storeIndex3.close()
  expect(filesIndex.sideEffects).toBeTruthy()
  expect(filesIndex.sideEffects?.has(`${ENGINE_NAME};format=2;deps=${hashObject({
    id: `@pnpm/postinstall-modifies-source@1.0.0:${getIntegrity('@pnpm/postinstall-modifies-source', '1.0.0')}`,
    deps: {},
  })}`)).toBeTruthy()
  const sideEffectEntry = filesIndex.sideEffects!.get(`${ENGINE_NAME};format=2;deps=${hashObject({
    id: `@pnpm/postinstall-modifies-source@1.0.0:${getIntegrity('@pnpm/postinstall-modifies-source', '1.0.0')}`,
    deps: {},
  })}`)!
  const patchedFileDigest = sideEffectEntry.added!.get('empty-file.txt')?.digest
  expect(patchedFileDigest).toBeTruthy()
  const originalFileDigest = filesIndex.files.get('empty-file.txt')!.digest
  expect(originalFileDigest).toBeTruthy()
  // The digest of the original file differs from the digest of the patched file
  expect(originalFileDigest).not.toEqual(patchedFileDigest)

  expect(fs.readFileSync(getFilePathByModeInCafs(opts.storeDir, originalFileDigest, 420), 'utf8')).toBe('')
})

test('a corrupted side-effects cache is ignored', async () => {
  prepareEmpty()

  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0'], opts)

  const filesIndexKey4 = storeIndexKey(getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const storeIndex4 = new StoreIndex(opts.storeDir)
  const filesIndex4 = storeIndex4.get(filesIndexKey4) as PackageFilesIndex
  storeIndex4.close()
  expect(filesIndex4.sideEffects).toBeTruthy() // files index has side effects
  const sideEffectsKey = `${ENGINE_NAME};format=2;deps=${hashObject({
    id: `@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0:${getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')}`,
    deps: {
      '@pnpm.e2e/hello-world-js-bin': hashObject({
        id: `@pnpm.e2e/hello-world-js-bin@1.0.0:${getIntegrity('@pnpm.e2e/hello-world-js-bin', '1.0.0')}`,
        deps: {},
      }),
    },
  })}`

  expect(filesIndex4.sideEffects).toBeTruthy()
  expect(filesIndex4.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex4.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  expect(filesIndex4.sideEffects!.get(sideEffectsKey)!.added!.has('generated-by-preinstall.js')).toBeTruthy()
  const sideEffectFileStat = filesIndex4.sideEffects!.get(sideEffectsKey)!.added!.get('generated-by-preinstall.js')!
  const sideEffectFile = getFilePathByModeInCafs(opts.storeDir, sideEffectFileStat.digest, sideEffectFileStat.mode)
  expect(fs.existsSync(sideEffectFile)).toBeTruthy()
  rimrafSync(sideEffectFile) // we remove the side effect file to break the store

  rimrafSync('node_modules')
  const opts2 = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    storeDir: opts.storeDir,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  })
  await install(manifest, opts2)

  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy() // side effects cache correctly used
})

// https://github.com/pnpm/pnpm/issues/12859
const testOnPosix = process.platform === 'win32' ? test.skip : test
testOnPosix('symlinks created by a build are restored from the side-effects cache', async () => {
  prepareEmpty()
  const pkgName = '@pnpm.e2e/postinstall-creates-symlinks'
  const pkgDir = `node_modules/${pkgName}`
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    allowBuilds: { [pkgName]: true },
  })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, [`${pkgName}@1.0.0`], opts)

  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(storeIndexKey(getIntegrity(pkgName, '1.0.0'), `${pkgName}@1.0.0`)) as PackageFilesIndex
  const added = [...filesIndex.sideEffects!.values()][0].added!
  expect(isSymlinkMode(added.get('bin/tool-alias')!.mode)).toBe(true)
  expect(isSymlinkMode(added.get('lib-link')!.mode)).toBe(true)
  expect(isSymlinkMode(added.get('replaced.js')!.mode)).toBe(true)

  rimrafSync('node_modules')
  process.env.PNPM_E2E_FAIL_POSTINSTALL = '1'
  try {
    await install(manifest, testDefaults({
      fastUnpack: false,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
      storeDir: opts.storeDir,
      allowBuilds: { [pkgName]: true },
    }))
  } finally {
    delete process.env.PNPM_E2E_FAIL_POSTINSTALL
  }

  expect(fs.readlinkSync(`${pkgDir}/bin/tool-alias`)).toBe('tool')
  expect(fs.readlinkSync(`${pkgDir}/lib-link`)).toBe('lib')
  expect(fs.readFileSync(`${pkgDir}/lib-link/index.js`, 'utf8')).toBe('module.exports = true\n')
  expect(fs.readlinkSync(`${pkgDir}/replaced.js`)).toBe('lib/index.js')
})

// https://github.com/pnpm/pnpm/issues/15667
test('a build with nothing to restore runs again when the package ships an executable file', async () => {
  prepareEmpty()
  const log = path.resolve('outside-log')
  fs.writeFileSync(log, '')
  process.env.PNPM_E2E_OUTSIDE_LOG = log
  try {
    const opts = testDefaults({
      fastUnpack: false,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
      allowBuilds: { '@pnpm.e2e/postinstall-writes-outside-package': true },
    })
    const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/postinstall-writes-outside-package@1.1.0'], opts)
    expect(fs.readFileSync(log, 'utf8')).toBe('x')

    rimrafSync('node_modules')
    await install(manifest, testDefaults({
      fastUnpack: false,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
      storeDir: opts.storeDir,
      allowBuilds: { '@pnpm.e2e/postinstall-writes-outside-package': true },
    }))
    expect(fs.readFileSync(log, 'utf8')).toBe('xx')
  } finally {
    delete process.env.PNPM_E2E_OUTSIDE_LOG
  }
})
