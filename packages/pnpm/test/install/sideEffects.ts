import prepare from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { existsSync, promises as fs } from 'fs'
import { execPnpm } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')

const ENGINE_DIR = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

test.skip('caching side effects of native package', async function () {
  const project = prepare()

  await execPnpm(['add', '--side-effects-cache', 'diskusage@1.1.3'])
  const storePath = await project.getStorePath()
  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  const stat1 = await fs.stat(cacheBuildDir)

  expect(existsSync(path.join('node_modules/diskusage/build'))).toBeTruthy()
  expect(existsSync(cacheBuildDir)).toBeTruthy()

  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache'])
  const stat2 = await fs.stat(cacheBuildDir)
  expect(stat1.ino).toBe(stat2.ino)

  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--force'])
  const stat3 = await fs.stat(cacheBuildDir)
  expect(stat1.ino).not.toBe(stat3.ino)
})

test.skip('using side effects cache', async function () {
  const project = prepare()

  // Right now, hardlink does not work with side effects, so we specify copy as the packageImportMethod
  // We disable verifyStoreIntegrity because we are going to change the cache
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])
  const storePath = await project.getStorePath()

  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  expect(existsSync('node_modules/diskusage/build/new-file.txt')).toBeTruthy()
})

test.skip('readonly side effects cache', async function () {
  const project = prepare()

  await execPnpm(['add', 'diskusage@1.1.2', '--side-effects-cache', '--no-verify-store-integrity'])
  const storePath = await project.getStorePath()

  // Modify the side effects cache to make sure we are using it
  const cacheBuildDir = path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.2/side_effects/${ENGINE_DIR}/package/build`)
  await fs.writeFile(path.join(cacheBuildDir, 'new-file.txt'), 'some new content')

  await rimraf('node_modules')
  await execPnpm(['add', 'diskusage@1.1.2', '--side-effects-cache-readonly', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  expect(existsSync('node_modules/diskusage/build/new-file.txt')).toBeTruthy()

  await rimraf('node_modules')
  // changing version to make sure we don't create the cache
  await execPnpm(['add', 'diskusage@1.1.3', '--side-effects-cache-readonly', '--no-verify-store-integrity', '--package-import-method', 'copy'])

  expect(existsSync('node_modules/diskusage/build')).toBeTruthy()
  expect(existsSync(path.join(storePath, `localhost+${REGISTRY_MOCK_PORT}/diskusage/1.1.3/side_effects/${ENGINE_DIR}/package/build`))).not.toBeTruthy()
})
