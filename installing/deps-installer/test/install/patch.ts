import fs from 'node:fs'
import path from 'node:path'

import { afterAll, expect, jest, test } from '@jest/globals'
import { ENGINE_NAME } from '@pnpm/constants'
import { createHexHashFromFile } from '@pnpm/crypto.hash'
import { install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { fixtures } from '@pnpm/test-fixtures'
import { rimrafSync } from '@zkochan/rimraf'

import { testDefaults } from '../utils/index.js'

const f = fixtures(import.meta.dirname)

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

test('patch package with exact version', async () => {
  const reporter = jest.fn()
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
  }
  const opts = testDefaults({
    allowBuilds: {},
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
    reporter,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    packageNames: [],
    level: 'debug',
    name: 'pnpm:ignored-scripts',
  }))

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': patchFileHash,
  })
  expect(lockfile.snapshots[`is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexKey = storeIndexKey(getIntegrity('is-positive', '1.0.0'), 'is-positive@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeTruthy()
  const sideEffectsKey = `${ENGINE_NAME};patch=${patchFileHash}`
  expect(filesIndex.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  const patchedFileDigest = filesIndex.sideEffects!.get(sideEffectsKey)!.added!.get('index.js')?.digest
  expect(patchedFileDigest).toBeTruthy()
  const originalFileDigest = filesIndex.files.get('index.js')!.digest
  expect(originalFileDigest).toBeTruthy()
  // The digest of the original file differs from the digest of the patched file
  expect(originalFileDigest).not.toEqual(patchedFileDigest)

  // The same with frozen lockfile
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  // The same with frozen lockfile and hoisted node_modules
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
    nodeLinker: 'hoisted',
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  process.chdir('..')
  fs.mkdirSync('project2')
  process.chdir('project2')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    offline: true,
  }, {}, {}, { packageImportMethod: 'hardlink' }))

  // The original file did not break, when a patched version was created
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package with version range', async () => {
  const reporter = jest.fn()
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1': patchPath,
  }
  const opts = testDefaults({
    allowBuilds: {},
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
    reporter,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(reporter).toHaveBeenCalledWith(expect.objectContaining({
    packageNames: [],
    level: 'debug',
    name: 'pnpm:ignored-scripts',
  }))

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1': patchFileHash,
  })
  expect(lockfile.snapshots[`is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexKey = storeIndexKey(getIntegrity('is-positive', '1.0.0'), 'is-positive@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeTruthy()
  const sideEffectsKey = `${ENGINE_NAME};patch=${patchFileHash}`
  expect(filesIndex.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  const patchedFileDigest = filesIndex.sideEffects!.get(sideEffectsKey)!.added!.get('index.js')?.digest
  expect(patchedFileDigest).toBeTruthy()
  const originalFileDigest = filesIndex.files.get('index.js')!.digest
  expect(originalFileDigest).toBeTruthy()
  // The digest of the original file differs from the digest of the patched file
  expect(originalFileDigest).not.toEqual(patchedFileDigest)

  // The same with frozen lockfile
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  // The same with frozen lockfile and hoisted node_modules
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
    nodeLinker: 'hoisted',
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  process.chdir('..')
  fs.mkdirSync('project2')
  process.chdir('project2')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    offline: true,
  }, {}, {}, { packageImportMethod: 'hardlink' }))

  // The original file did not break, when a patched version was created
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package reports warning if not all patches are applied and allowUnusedPatches is set', async () => {
  prepareEmpty()
  const reporter = jest.fn()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
    'is-negative@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
    allowUnusedPatches: true,
    reporter,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)
  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: 'The following patches were not used: is-negative@1.0.0',
    })
  )
})

test('patch package throws an exception if not all patches are applied', async () => {
  prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
    'is-negative@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await expect(
    install({
      dependencies: {
        'is-positive': '1.0.0',
      },
    }, opts)
  ).rejects.toThrow('The following patches were not used: is-negative@1.0.0')
})

test('the patched package is updated if the patch is modified', async () => {
  prepareEmpty()
  f.copy('patch-pkg', 'patches')
  const patchPath = path.resolve('patches', 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  const manifest = {
    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  await install(manifest, opts)

  const patchContent = fs.readFileSync(patchPath, 'utf8')
  fs.writeFileSync(patchPath, patchContent.replace('// patched', '// edited patch'), 'utf8')

  await install(manifest, opts)
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// edited patch')
})

test('patch package when scripts are ignored', async () => {
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    ignoreScripts: true,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': patchFileHash,
  })
  expect(lockfile.snapshots[`is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexKey = storeIndexKey(getIntegrity('is-positive', '1.0.0'), 'is-positive@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeTruthy()
  const sideEffectsKey = `${ENGINE_NAME};patch=${patchFileHash}`
  expect(filesIndex.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  const patchedFileDigest = filesIndex.sideEffects!.get(sideEffectsKey)!.added!.get('index.js')?.digest
  expect(patchedFileDigest).toBeTruthy()
  const originalFileDigest = filesIndex.files.get('index.js')!.digest
  expect(originalFileDigest).toBeTruthy()
  // The digest of the original file differs from the digest of the patched file
  expect(originalFileDigest).not.toEqual(patchedFileDigest)

  // The same with frozen lockfile
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  // The same with frozen lockfile and hoisted node_modules
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
    nodeLinker: 'hoisted',
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  process.chdir('..')
  fs.mkdirSync('project2')
  process.chdir('project2')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    fastUnpack: false,
    ignoreScripts: true,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    offline: true,
  }, {}, {}, { packageImportMethod: 'hardlink' }))

  // The original file did not break, when a patched version was created
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package when the package is not in allowBuilds list', async () => {
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
    allowBuilds: {},
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': patchFileHash,
  })
  expect(lockfile.snapshots[`is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexKey = storeIndexKey(getIntegrity('is-positive', '1.0.0'), 'is-positive@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeTruthy()
  const sideEffectsKey = `${ENGINE_NAME};patch=${patchFileHash}`
  expect(filesIndex.sideEffects!.has(sideEffectsKey)).toBeTruthy()
  expect(filesIndex.sideEffects!.get(sideEffectsKey)!.added).toBeTruthy()
  const patchedFileDigest = filesIndex.sideEffects!.get(sideEffectsKey)!.added!.get('index.js')?.digest
  expect(patchedFileDigest).toBeTruthy()
  const originalFileDigest = filesIndex.files.get('index.js')!.digest
  expect(originalFileDigest).toBeTruthy()
  // The digest of the original file differs from the digest of the patched file
  expect(originalFileDigest).not.toEqual(patchedFileDigest)

  // The same with frozen lockfile
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  // The same with frozen lockfile and hoisted node_modules
  rimrafSync('node_modules')
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, {
    ...opts,
    frozenLockfile: true,
    nodeLinker: 'hoisted',
  })
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  process.chdir('..')
  fs.mkdirSync('project2')
  process.chdir('project2')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    allowBuilds: {},
    offline: true,
  }, {}, {}, { packageImportMethod: 'hardlink' }))

  // The original file did not break, when a patched version was created
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package when the patched package has no dependencies and appears multiple times', async () => {
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
    overrides: {
      'is-positive': '1.0.0',
    },
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
      'is-not-positive': '1.0.0',
    },
  }, opts)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = await createHexHashFromFile(patchPath)
  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.snapshots).sort()).toStrictEqual([
    'is-not-positive@1.0.0',
    `is-positive@1.0.0(patch_hash=${patchFileHash})`,
  ].sort())
})

test('patch package should fail when the exact version patch fails to apply', async () => {
  prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@3.1.0': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await expect(install({
    dependencies: {
      'is-positive': '3.1.0',
    },
  }, opts)).rejects.toThrow(/Could not apply patch/)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package should fail when the version range patch fails to apply', async () => {
  prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@>=3': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await expect(install({
    dependencies: {
      'is-positive': '3.1.0',
    },
  }, opts)).rejects.toThrow(/Could not apply patch/)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})

test('patch package should fail when the name-only range patch fails to apply', async () => {
  prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive': patchPath,
  }
  const opts = testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    patchedDependencies,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await expect(install({
    dependencies: {
      'is-positive': '3.1.0',
    },
  }, opts)).rejects.toThrow(/Could not apply patch/)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})
