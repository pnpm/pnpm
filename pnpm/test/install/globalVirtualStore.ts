import fs from 'node:fs'
import path from 'node:path'

import { STORE_VERSION } from '@pnpm/constants'
import { prepare } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test('using a global virtual store', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const globalVirtualStoreDir = path.join(storeDir, 'v11/links')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })
  await execPnpm(['install'])

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
  const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files).toHaveLength(1)
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('approve-builds updates GVS symlinks and runs builds at correct hash directory', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const globalVirtualStoreDir = path.join(storeDir, 'v11/links')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
  })

  // Step 1: Install with GVS, builds NOT approved
  await execPnpm(['install', '--config.strict-dep-builds=false'])

  const pkgVersionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example/1.0.0')
  const hashBefore = fs.readdirSync(pkgVersionDir)
  expect(hashBefore).toHaveLength(1)

  // Build artifacts should NOT be present
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeFalsy()

  // Step 2: approve-builds — updates config then runs install in GVS mode
  await execPnpm(['approve-builds', '--all'])

  // Step 3: Verify GVS hash changed (new engine-specific directory)
  const hashesAfter = fs.readdirSync(pkgVersionDir)
  const newHash = hashesAfter.find((h) => h !== hashBefore[0])
  expect(newHash).toBeDefined()
  expect(newHash).not.toBe(hashBefore[0])

  // Build artifacts should be in the new hash directory
  const newPkgDir = path.join(pkgVersionDir, newHash!, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-preinstall.js'))).toBeTruthy()

  // Build artifacts should be accessible through node_modules
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')).toBeTruthy()
  expect(fs.existsSync('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')).toBeTruthy()

  // allowBuilds should be persisted in workspace manifest
  const workspaceManifest = readYamlFileSync<any>(path.resolve('pnpm-workspace.yaml')) // eslint-disable-line
  expect(workspaceManifest.allowBuilds?.['@pnpm.e2e/pre-and-postinstall-scripts-example']).toBe(true)
})

test('GVS path cache produces same node_modules on reinstall', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const gvsCacheDir = path.join(storeDir, STORE_VERSION, '.pnpm-gvs-paths')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })

  // First install — resolves deps, populates GVS store
  await execPnpm(['install'])
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()

  // Delete node_modules → frozen-lockfile triggers headless path, writes cache
  fs.rmSync(path.resolve('node_modules'), { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])

  // Verify cache was written
  expect(fs.existsSync(gvsCacheDir)).toBeTruthy()
  const cacheFiles = fs.readdirSync(gvsCacheDir)
  expect(cacheFiles.length).toBeGreaterThan(0)

  // Delete node_modules again → frozen-lockfile should use cached paths (cache hit)
  fs.rmSync(path.resolve('node_modules'), { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])

  // Verify node_modules is correct (proves cache hit path works)
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
})

test('GVS path cache invalidates on lockfile change', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  const gvsCacheDir = path.join(storeDir, STORE_VERSION, '.pnpm-gvs-paths')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })

  // First install → delete node_modules → frozen install writes cache
  await execPnpm(['install'])
  fs.rmSync(path.resolve('node_modules'), { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])
  expect(fs.existsSync(gvsCacheDir)).toBeTruthy()
  const cacheFilesBefore = fs.readdirSync(gvsCacheDir)
  expect(cacheFilesBefore.length).toBeGreaterThan(0)

  // Add a new dependency — changes lockfile.packages
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  manifest.dependencies['is-positive'] = '1.0.0'
  fs.writeFileSync('package.json', JSON.stringify(manifest))

  // Resolve new dep, then frozen install writes new cache with different hash
  await execPnpm(['install'])
  fs.rmSync(path.resolve('node_modules'), { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])

  // Verify new cache file exists (different hash)
  const cacheFilesAfter = fs.readdirSync(gvsCacheDir)
  expect(cacheFilesAfter.length).toBeGreaterThan(cacheFilesBefore.length)

  // Verify both packages are present
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/is-positive/package.json'))).toBeTruthy()
})
