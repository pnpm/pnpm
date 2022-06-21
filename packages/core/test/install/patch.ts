import fs from 'fs'
import path from 'path'
import { PackageFilesIndex } from '@pnpm/cafs'
import { ENGINE_NAME } from '@pnpm/constants'
import { install } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import fixtures from '@pnpm/test-fixtures'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('patch package', async () => {
  const project = prepareEmpty()
  const patchPath = path.join(f.find('patch-pkg'), 'is-positive@1.0.0.patch')

  const patchedDependencies = {
    'is-positive@1.0.0': path.relative(process.cwd(), patchPath),
  }
  const opts = await testDefaults({
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
  const lockfile = await project.readLockfile()
  expect(lockfile.patchedDependencies).toStrictEqual(patchedDependencies)
  expect(lockfile.packages[`/is-positive/1.0.0_${patchFileHash}`]).toBeTruthy()

  const filesIndexFile = path.join(opts.storeDir, 'files/c7/1ccf199e0fdae37aad13946b937d67bcd35fa111b84d21b3a19439cfdc2812c5d8da8a735e94c2a1ccb77b4583808ee8405313951e7146ac83ede3671dc292-index.json')
  const filesIndex = await loadJsonFile<PackageFilesIndex>(filesIndexFile)
  const sideEffectsKey = `${ENGINE_NAME}-{}-${patchFileHash}`
  const patchedFileIntegrity = filesIndex.sideEffects?.[sideEffectsKey]['index.js']?.integrity
  expect(patchedFileIntegrity).toBeTruthy()
  const originalFileIntegrity = filesIndex.files['index.js'].integrity
  expect(originalFileIntegrity).toBeTruthy()
  // The integrity of the original file differs from the integrity of the patched file
  expect(originalFileIntegrity).not.toEqual(patchedFileIntegrity)

  // The same with frozen lockfile
  await rimraf('node_modules')
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
  await rimraf('node_modules')
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
  }, await testDefaults({
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    offline: true,
  }, {}, {}, { packageImportMethod: 'hardlink' }))

  // The original file did not break, when a patched version was created
  expect(fs.readFileSync('node_modules/is-positive/index.js', 'utf8')).not.toContain('// patched')
})
