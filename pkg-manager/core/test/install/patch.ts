import fs from 'fs'
import path from 'path'
import { type PackageFilesIndex } from '@pnpm/store.cafs'
import { ENGINE_NAME } from '@pnpm/constants'
import { install } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as rimraf } from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('patch package', async () => {
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
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = 'jnbpamcxayl5i4ehrkoext3any'
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': {
      path: path.relative(process.cwd(), patchedDependencies['is-positive@1.0.0']).replaceAll('\\', '/'),
      hash: patchFileHash,
    },
  })
  expect(lockfile.snapshots[`/is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexFile = path.join(opts.storeDir, 'files/c7/1ccf199e0fdae37aad13946b937d67bcd35fa111b84d21b3a19439cfdc2812c5d8da8a735e94c2a1ccb77b4583808ee8405313951e7146ac83ede3671dc292-index.json')
  const filesIndex = loadJsonFile.sync<PackageFilesIndex>(filesIndexFile)
  const sideEffectsKey = `${ENGINE_NAME}-${patchFileHash}`
  const patchedFileIntegrity = filesIndex.sideEffects?.[sideEffectsKey]['index.js']?.integrity
  expect(patchedFileIntegrity).toBeTruthy()
  const originalFileIntegrity = filesIndex.files['index.js'].integrity
  expect(originalFileIntegrity).toBeTruthy()
  // The integrity of the original file differs from the integrity of the patched file
  expect(originalFileIntegrity).not.toEqual(patchedFileIntegrity)

  // The same with frozen lockfile
  rimraf('node_modules')
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
  rimraf('node_modules')
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

test('patch package reports warning if not all patches are applied and allowNonAppliedPatches is set', async () => {
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
    allowNonAppliedPatches: true,
    reporter,
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)
  expect(reporter).toBeCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: 'The following patches were not applied: is-negative@1.0.0',
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
  ).rejects.toThrow('The following patches were not applied: is-negative@1.0.0')
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

  const patchFileHash = 'jnbpamcxayl5i4ehrkoext3any'
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': {
      path: path.relative(process.cwd(), patchedDependencies['is-positive@1.0.0']).replaceAll('\\', '/'),
      hash: patchFileHash,
    },
  })
  expect(lockfile.snapshots[`/is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexFile = path.join(opts.storeDir, 'files/c7/1ccf199e0fdae37aad13946b937d67bcd35fa111b84d21b3a19439cfdc2812c5d8da8a735e94c2a1ccb77b4583808ee8405313951e7146ac83ede3671dc292-index.json')
  const filesIndex = loadJsonFile.sync<PackageFilesIndex>(filesIndexFile)
  const sideEffectsKey = `${ENGINE_NAME}-${patchFileHash}`
  const patchedFileIntegrity = filesIndex.sideEffects?.[sideEffectsKey]['index.js']?.integrity
  expect(patchedFileIntegrity).toBeTruthy()
  const originalFileIntegrity = filesIndex.files['index.js'].integrity
  expect(originalFileIntegrity).toBeTruthy()
  // The integrity of the original file differs from the integrity of the patched file
  expect(originalFileIntegrity).not.toEqual(patchedFileIntegrity)

  // The same with frozen lockfile
  rimraf('node_modules')
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
  rimraf('node_modules')
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

test('patch package when the package is not in onlyBuiltDependencies list', async () => {
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
    onlyBuiltDependencies: [],
  }, {}, {}, { packageImportMethod: 'hardlink' })
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, opts)

  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).toContain('// patched')

  const patchFileHash = 'jnbpamcxayl5i4ehrkoext3any'
  const lockfile = project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual({
    'is-positive@1.0.0': {
      path: path.relative(process.cwd(), patchedDependencies['is-positive@1.0.0']).replaceAll('\\', '/'),
      hash: patchFileHash,
    },
  })
  expect(lockfile.snapshots[`/is-positive@1.0.0(patch_hash=${patchFileHash})`]).toBeTruthy()

  const filesIndexFile = path.join(opts.storeDir, 'files/c7/1ccf199e0fdae37aad13946b937d67bcd35fa111b84d21b3a19439cfdc2812c5d8da8a735e94c2a1ccb77b4583808ee8405313951e7146ac83ede3671dc292-index.json')
  const filesIndex = loadJsonFile.sync<PackageFilesIndex>(filesIndexFile)
  const sideEffectsKey = `${ENGINE_NAME}-${patchFileHash}`
  const patchedFileIntegrity = filesIndex.sideEffects?.[sideEffectsKey]['index.js']?.integrity
  expect(patchedFileIntegrity).toBeTruthy()
  const originalFileIntegrity = filesIndex.files['index.js'].integrity
  expect(originalFileIntegrity).toBeTruthy()
  // The integrity of the original file differs from the integrity of the patched file
  expect(originalFileIntegrity).not.toEqual(patchedFileIntegrity)

  // The same with frozen lockfile
  rimraf('node_modules')
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
  rimraf('node_modules')
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
    onlyBuiltDependencies: [],
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

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.snapshots).sort()).toStrictEqual([
    '/is-not-positive@1.0.0',
    '/is-positive@1.0.0(patch_hash=jnbpamcxayl5i4ehrkoext3any)',
  ].sort())
})

test('patch package should fail when the patch could not be applied', async () => {
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
