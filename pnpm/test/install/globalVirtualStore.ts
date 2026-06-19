import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
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

test('warm GVS reinstall skips internal linking', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '*',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })

  // First install — warms GVS store
  await execPnpm(['install'])

  // Delete node_modules
  fs.rmSync(path.resolve('node_modules'), { recursive: true })

  // Reinstall with frozen lockfile — should skip internal GVS linking
  await execPnpm(['install', '--frozen-lockfile'])

  // Verify structure is correct after warm reinstall
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/hello-world-js-bin/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.bin/hello-world-js-bin'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
})

test('switching from non-GVS to GVS replaces stale hoisted symlinks', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')

  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    storeDir,
    privateHoistPattern: '*',
  })
  await execPnpm(['install'])

  const depOfPkgWith1Dep = path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
  expect(fs.existsSync(depOfPkgWith1Dep)).toBeTruthy()
  const oldTarget = path.resolve(path.dirname(depOfPkgWith1Dep), fs.readlinkSync(depOfPkgWith1Dep))
  expect(oldTarget).toContain('.pnpm')

  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })
  await execPnpm(['install', '--config.confirmModulesPurge=false'])

  const newTarget = path.resolve(path.dirname(depOfPkgWith1Dep), fs.readlinkSync(depOfPkgWith1Dep))
  expect(newTarget).toContain(path.join(storeDir, 'v11', 'links'))

  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeTruthy()

  const globalVirtualStoreDir = path.join(storeDir, 'v11/links')
  const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files).toHaveLength(1)
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('the post-install build step preserves the global virtual store directory of a workspace package', async () => {
  // A workspace package that is also its own workspace root (its own
  // pnpm-workspace.yaml and lockfile). The root install runs a per-project
  // build pass that must not overwrite the package's recorded global virtual
  // store directory with the local node_modules/.pnpm — otherwise the next
  // install in that package detects a virtual-store mismatch and prompts to
  // purge node_modules.
  const storeDir = path.resolve('store')
  const globalVirtualStoreDir = path.join(storeDir, 'v11/links')
  preparePackages([
    {
      location: 'libs/common',
      package: {
        name: '@repro/common',
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        },
      },
    },
  ])
  writeYamlFileSync(path.resolve('pnpm-workspace.yaml'), {
    packages: ['libs/*'],
    enableGlobalVirtualStore: true,
    sharedWorkspaceLockfile: false,
    storeDir,
  })
  writeYamlFileSync(path.resolve('libs/common/pnpm-workspace.yaml'), {
    enableGlobalVirtualStore: true,
    storeDir,
  })

  await execPnpm(['install'])

  const modulesManifestPath = path.resolve('libs/common/node_modules/.modules.yaml')
  const modules = readYamlFileSync<{ virtualStoreDir: string }>(modulesManifestPath)
  expect(path.resolve('libs/common/node_modules', modules.virtualStoreDir)).toBe(globalVirtualStoreDir)

  // A subsequent install in the package must be a no-op, not a virtual-store
  // mismatch that aborts (in a non-TTY shell) or prompts to purge node_modules.
  await execPnpm(['install', '--dir', path.resolve('libs/common')])
})
