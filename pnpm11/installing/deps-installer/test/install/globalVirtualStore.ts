import fs from 'node:fs'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { install, type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { addDistTag, getIntegrity } from '@pnpm/testing.registry-mock'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'

import { testDefaults } from '../utils/index.js'

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

function getGlobalVirtualStoreSlots (globalVirtualStoreDir: string, name: string, version: string): string[] {
  const slots: string[] = []
  const legacyVersionDir = path.join(globalVirtualStoreDir, name, version)
  if (fs.existsSync(legacyVersionDir)) {
    slots.push(...fs.readdirSync(legacyVersionDir).map((hash) => path.join(legacyVersionDir, hash)))
  }
  const contextsDir = path.join(globalVirtualStoreDir, 'contexts')
  if (fs.existsSync(contextsDir)) {
    for (const contextHash of fs.readdirSync(contextsDir)) {
      const versionDir = path.join(contextsDir, contextHash, name, version)
      if (fs.existsSync(versionDir)) {
        slots.push(...fs.readdirSync(versionDir).map((hash) => path.join(versionDir, hash)))
      }
    }
  }
  return slots.sort()
}

test('using a global virtual store', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  }))

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
    expect(slots).toHaveLength(1)
    expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  }

  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    frozenLockfile: true,
    hoistPattern: ['*'],
  }))

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
    expect(slots).toHaveLength(1)
    expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  }
})

test('TypeScript resolves consumer-provided types from global virtual store packages', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  await install({
    dependencies: {
      'next-themes': '0.3.0',
      react: '18.3.1',
      'react-dom': '18.3.1',
    },
    devDependencies: {
      '@types/react': '18.3.3',
      typescript: '5.5.4',
    },
  }, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  }))

  fs.writeFileSync('index.ts', `
import { useTheme } from 'next-themes'

type IsAny<T> = 0 extends (1 & T) ? true : false
type AssertFalse<T extends false> = T

const { setTheme } = useTheme()
type SetThemeIsAny = IsAny<typeof setTheme>
type Check = AssertFalse<SetThemeIsAny>
  `)

  await execa(process.execPath, [
    'node_modules/typescript/bin/tsc',
    '--noEmit',
    '--skipLibCheck',
    '--strict',
    '--moduleResolution',
    'bundler',
    '--module',
    'esnext',
    'index.ts',
  ])
})

test('GVS packages resolve arbitrary project dependencies with native ESM and CommonJS', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    hoistPattern: [],
    publicHoistPattern: [],
    virtualStoreDir: globalVirtualStoreDir,
  })
  const assertResolution = async () => {
    const packageDir = fs.realpathSync('node_modules/@pnpm.e2e/pkg-with-1-dep')
    const esmProbe = path.join(packageDir, 'gvs-resolution-probe.mjs')
    const cjsProbe = path.join(packageDir, 'gvs-resolution-probe.cjs')
    fs.writeFileSync(esmProbe, "import.meta.resolve('@pnpm.e2e/foo/package.json')\n")
    fs.writeFileSync(cjsProbe, "require.resolve('@pnpm.e2e/foo/package.json')\n")
    await execa(process.execPath, [esmProbe])
    await execa(process.execPath, [cjsProbe])
    const contextHash = path.relative(globalVirtualStoreDir, packageDir).split(path.sep)[1]
    expect(fs.existsSync(path.join(
      globalVirtualStoreDir,
      'contexts',
      contextHash,
      'node_modules/.bin/hello-world-js-bin'
    ))).toBeTruthy()
  }

  await install(manifest, opts)
  await assertResolution()

  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)
  await install(manifest, {
    ...opts,
    frozenLockfile: true,
  })
  await assertResolution()
})

test('reinstall from warm global virtual store after deleting node_modules', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  })
  await install(manifest, opts)

  for (const contextHash of fs.readdirSync(path.join(globalVirtualStoreDir, 'contexts'))) {
    rimrafSync(path.join(globalVirtualStoreDir, 'contexts', contextHash, 'node_modules'))
  }
  rimrafSync('node_modules')
  expect(fs.existsSync(globalVirtualStoreDir)).toBeTruthy()

  // Spy on fetchPackage to verify the fast-path skips fetching
  const originalFetchPackage = opts.storeController.fetchPackage
  let fetchPackageCalls = 0
  opts.storeController.fetchPackage = ((fetchOpts) => {
    fetchPackageCalls++
    return originalFetchPackage(fetchOpts)
  }) as typeof originalFetchPackage

  // Reinstall with frozenLockfile — should reattach from the warm global store
  await install(manifest, {
    ...opts,
    frozenLockfile: true,
  })

  // fetchPackage should NOT be called — all packages reattached from warm GVS
  expect(fetchPackageCalls).toBe(0)

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(slots).toHaveLength(1)
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  const packageDir = fs.realpathSync('node_modules/@pnpm.e2e/pkg-with-1-dep')
  const probe = path.join(packageDir, 'warm-context-probe.mjs')
  fs.writeFileSync(probe, "import.meta.resolve('@pnpm.e2e/foo/package.json')\n")
  await execa(process.execPath, [probe])
})

test('modules are correctly updated when using a global virtual store', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      '@pnpm.e2e/peer-c': '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  })
  await install(manifest, opts)
  manifest.dependencies['@pnpm.e2e/peer-c'] = '2.0.0'
  await install(manifest, opts)

  {
    expect(fs.existsSync(path.resolve('node_modules/.pnpm/lock.yaml'))).toBeTruthy()
    const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/peer-c', '2.0.0')
    expect(slots).toHaveLength(1)
    expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/peer-c/package.json'))).toBeTruthy()
  }
})

test('GVS hashes are engine-agnostic for packages not in allowBuilds', async () => {
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }

  // Scenario 1: No packages allowed to build — all hashes should be engine-agnostic
  const gvsDir1 = path.resolve('links1')
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: gvsDir1,
    allowBuilds: {},
  }))
  rimrafSync('node_modules')

  // Scenario 2: Dependency allowed to build — parent hash becomes engine-specific
  // because it transitively depends on a package that is allowed to build
  const gvsDir2 = path.resolve('links2')
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: gvsDir2,
    frozenLockfile: true,
    allowBuilds: { '@pnpm.e2e/dep-of-pkg-with-1-dep': true },
  }))

  // Read hash directories for the parent package from both scenarios
  const slotNoBuilds = getGlobalVirtualStoreSlots(gvsDir1, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')[0]
  const slotWithBuilds = getGlobalVirtualStoreSlots(gvsDir2, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')[0]

  // Hashes must differ: scenario 1 omits ENGINE_NAME, scenario 2 includes it
  // (because dep-of-pkg-with-1-dep is allowed to build)
  expect(path.basename(slotNoBuilds)).not.toBe(path.basename(slotWithBuilds))

  // Both scenarios should still produce valid GVS layouts
  expect(fs.existsSync(path.join(slotNoBuilds, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(slotWithBuilds, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('GVS hashes are stable when allowBuilds targets an unrelated package', async () => {
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }

  // Scenario 1: No packages allowed to build
  const gvsDir1 = path.resolve('links1')
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: gvsDir1,
    allowBuilds: {},
  }))
  rimrafSync('node_modules')

  // Scenario 2: An unrelated package allowed to build
  // This should NOT affect hashes of @pnpm.e2e/pkg-with-1-dep or its deps
  const gvsDir2 = path.resolve('links2')
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: gvsDir2,
    frozenLockfile: true,
    allowBuilds: { 'some-unrelated-package': true },
  }))

  // Hashes should be identical since the allowBuilds target is not in the dep tree
  const hash1 = path.basename(getGlobalVirtualStoreSlots(gvsDir1, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')[0])
  const hash2 = path.basename(getGlobalVirtualStoreSlots(gvsDir2, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')[0])
  expect(hash1).toBe(hash2)
})

test('GVS re-links when allowBuilds changes', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }

  // Step 1: Install with no packages allowed to build (engine-agnostic hashes)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    allowBuilds: {},
  }))

  const slotBefore = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')[0]

  // Verify allowBuilds is stored in modules.yaml
  const rootModules = assertProject(process.cwd())
  const modulesState = rootModules.readModulesManifest()
  expect(modulesState?.allowBuilds).toEqual({})

  // Step 2: Reinstall with dep allowed to build — hashes should change
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    allowBuilds: { '@pnpm.e2e/dep-of-pkg-with-1-dep': true },
  }))

  const slotAfter = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
    .find((slot) => slot !== slotBefore)

  // A new hash directory should have been created
  expect(slotAfter).toBeDefined()
  expect(slotAfter).not.toBe(slotBefore)

  // Verify the new GVS layout is valid
  expect(fs.existsSync(path.join(slotAfter!, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()

  // Verify modules.yaml is updated with new allowBuilds
  const updatedState = rootModules.readModulesManifest()
  expect(updatedState?.allowBuilds).toEqual({ '@pnpm.e2e/dep-of-pkg-with-1-dep': true })
})

test('GVS successful build creates package directory with build artifacts', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  })
  await install(manifest, opts)

  // The GVS directory should exist with build artifacts
  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
  expect(slots).toHaveLength(1)
  const pkgInGvs = path.join(slots[0], 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(pkgInGvs, 'package.json'))).toBeTruthy()
  // Build artifacts created by postinstall script should be present
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-preinstall.js'))).toBeTruthy()
  // The .pnpm-needs-build marker should have been removed after successful build
  expect(fs.existsSync(path.join(pkgInGvs, '.pnpm-needs-build'))).toBeFalsy()

  // The .pnpm-needs-build marker must not be uploaded to the side effects cache
  const filesIndexKey = storeIndexKey(getIntegrity('@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0'), '@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0')
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  if (filesIndex.sideEffects) {
    for (const [, diff] of filesIndex.sideEffects) {
      expect(diff.added?.has('.pnpm-needs-build')).toBeFalsy()
    }
  }
})

test('GVS: approve-builds scenario — install with no builds, then reinstall with allowBuilds', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }

  // Step 1: Install with builds NOT approved (simulating first `pnpm install`)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: {},
  }))

  const slotsBefore = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
  expect(slotsBefore).toHaveLength(1)

  // Build artifacts should NOT be present
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeFalsy()

  // Step 2: Reinstall with allowBuilds changed (simulating what approve-builds does)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }))

  // Step 3: Verify the hash changed and build artifacts are in the new directory
  const newSlot = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
    .find((slot) => slot !== slotsBefore[0])
  expect(newSlot).toBeDefined()
  expect(newSlot).not.toBe(slotsBefore[0])

  // Build artifacts in new hash directory
  const newPkgDir = path.join(newSlot!, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-preinstall.js'))).toBeTruthy()

  // Build artifacts accessible via node_modules
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy()
})

test('GVS build failure cleans up broken package directory', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/failing-postinstall': '1.0.0',
    },
  }
  await expect(
    install(manifest, testDefaults({
      enableGlobalVirtualStore: true,
      virtualStoreDir: globalVirtualStoreDir,
      fastUnpack: false,
      allowBuilds: { '@pnpm.e2e/failing-postinstall': true },
    }))
  ).rejects.toThrow()

  // The GVS hash directory for the failed package should have been removed
  // on build failure so the next install can re-fetch and re-build.
  for (const slot of getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/failing-postinstall', '1.0.0')) {
    expect(fs.existsSync(path.join(slot, 'node_modules/@pnpm.e2e/failing-postinstall'))).toBeFalsy()
  }
})

test('GVS rebuilds successfully after simulated build failure cleanup', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }

  // Step 1: Successful install with build
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }))

  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
  expect(slots).toHaveLength(1)
  const hashDir = slots[0]
  expect(fs.existsSync(path.join(hashDir, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeTruthy()

  // Step 2: Simulate a previous build failure by removing the GVS hash directory
  rimrafSync(hashDir)
  expect(fs.existsSync(hashDir)).toBeFalsy()

  // Step 3: Remove node_modules and reinstall with frozenLockfile
  // The GVS fast path should NOT kick in because the hash dir is gone
  rimrafSync('node_modules')
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    frozenLockfile: true,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }))

  // The GVS directory should be recreated with build artifacts
  const slotsAfter = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
  expect(slotsAfter).toHaveLength(1)
  expect(fs.existsSync(path.join(slotsAfter[0], 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeTruthy()
})

test('GVS .pnpm-needs-build marker triggers re-import on next install', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pre-and-postinstall-scripts-example': '1.0.0',
    },
  }

  // Step 1: Install with build
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }))

  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example', '1.0.0')
  expect(slots).toHaveLength(1)
  const hashDir = slots[0]
  const pkgInGvs = path.join(hashDir, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeTruthy()
  // Marker should not be present after successful build
  expect(fs.existsSync(path.join(pkgInGvs, '.pnpm-needs-build'))).toBeFalsy()

  // Step 2: Simulate a crash between import and build — write a .pnpm-needs-build
  // marker and remove build artifacts (as if the build never completed)
  fs.writeFileSync(path.join(pkgInGvs, '.pnpm-needs-build'), '')
  fs.unlinkSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))
  expect(fs.existsSync(path.join(pkgInGvs, '.pnpm-needs-build'))).toBeTruthy()

  // Remove node_modules to force a re-install
  rimrafSync('node_modules')

  // Step 3: Reinstall — the GVS fast path should detect the .pnpm-needs-build
  // marker and force a re-fetch, re-import, and re-build.
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    frozenLockfile: true,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pre-and-postinstall-scripts-example': true },
  }))

  // The marker should be gone and the package rebuilt with artifacts
  expect(fs.existsSync(path.join(pkgInGvs, '.pnpm-needs-build'))).toBeFalsy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeTruthy()
})

test('injected local packages work with global virtual store', async () => {
  const project1Manifest = {
    name: 'project-1',
    version: '1.0.0',
    dependencies: {
      'is-positive': '1.0.0',
    },
  }
  const project2Manifest = {
    name: 'project-2',
    version: '1.0.0',
    dependencies: {
      'project-1': 'workspace:1.0.0',
    },
    dependenciesMeta: {
      'project-1': {
        injected: true,
      },
    },
  }
  preparePackages([
    {
      location: 'project-1',
      package: project1Manifest,
    },
    {
      location: 'project-2',
      package: project2Manifest,
    },
  ])
  fs.writeFileSync('project-1/foo.js', '', 'utf8')

  const globalVirtualStoreDir = path.resolve('links')
  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: project1Manifest,
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: project2Manifest,
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  await mutateModules(importers, testDefaults({
    autoInstallPeers: false,
    allProjects,
    enableGlobalVirtualStore: true,
    dedupeInjectedDeps: false,
    virtualStoreDir: globalVirtualStoreDir,
  }))

  // Verify project-2 has project-1 installed
  expect(fs.existsSync(path.resolve('project-2/node_modules/project-1'))).toBeTruthy()

  // Verify the modules manifest has injectedDeps pointing to global virtual store
  const rootModules = assertProject(process.cwd())
  const modulesState = rootModules.readModulesManifest()
  expect(modulesState?.injectedDeps?.['project-1']).toBeDefined()
  expect(modulesState?.injectedDeps?.['project-1'].length).toBeGreaterThan(0)
  // Injected deps should be in the global virtual store (links directory)
  const injectedDepLocation = modulesState?.injectedDeps?.['project-1'][0]
  expect(injectedDepLocation).toContain('links')
  expect(fs.existsSync(path.join(injectedDepLocation!, 'foo.js'))).toBeTruthy()
})

test('virtualStoreOnly populates standard virtual store without importer symlinks', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await install(manifest, testDefaults({
    virtualStoreOnly: true,
  }))

  // Standard virtual store should be populated
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

  // Importer-level symlinks should NOT exist
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
})

test('virtualStoreOnly with enableModulesDir=false throws config error (standard virtual store)', async () => {
  prepareEmpty()
  await expect(
    install({}, testDefaults({
      virtualStoreOnly: true,
      enableModulesDir: false,
    }))
  ).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR',
  })
})

test('virtualStoreOnly with enableModulesDir=false works when GVS is enabled', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('gvs-no-modules')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  // First install to generate lockfile (with modules dir enabled)
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  }))

  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)

  // Now install with virtualStoreOnly + enableModulesDir=false + GVS — should NOT throw
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    virtualStoreOnly: true,
    enableModulesDir: false,
    frozenLockfile: true,
  }))

  // GVS should be populated
  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(slots).toHaveLength(1)
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('virtualStoreOnly with GVS populates global virtual store without importer links', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('gvs')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    virtualStoreOnly: true,
  }))

  // GVS should be populated
  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(slots).toHaveLength(1)
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

  // Importer-level links should NOT exist
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
  // No hoisted deps
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeFalsy()
  // No bin links
  expect(fs.existsSync(path.resolve('node_modules/.bin'))).toBeFalsy()
  const contextHash = path.relative(globalVirtualStoreDir, slots[0]).split(path.sep)[1]
  expect(fs.existsSync(path.join(globalVirtualStoreDir, 'contexts', contextHash, 'node_modules'))).toBeFalsy()
})

test('virtualStoreOnly with frozenLockfile populates virtual store without importer symlinks', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('gvs-frozen')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  // First install to generate lockfile
  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  }))

  // Remove node_modules and GVS, then reinstall with frozenLockfile + virtualStoreOnly
  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)

  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    virtualStoreOnly: true,
    frozenLockfile: true,
  }))

  // GVS should be populated
  const slots = getGlobalVirtualStoreSlots(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(slots).toHaveLength(1)
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  // Transitive dependency should also be in GVS
  expect(fs.existsSync(path.join(slots[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

  // Importer-level symlinks should NOT exist
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
  // No hoisted deps
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeFalsy()
  // No bin links
  expect(fs.existsSync(path.resolve('node_modules/.bin'))).toBeFalsy()
})

test('virtualStoreOnly with frozenLockfile populates standard virtual store without importer symlinks', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  // First install to generate lockfile
  await install(manifest, testDefaults())

  // Remove node_modules, then reinstall with frozenLockfile + virtualStoreOnly
  rimrafSync('node_modules')

  await install(manifest, testDefaults({
    virtualStoreOnly: true,
    frozenLockfile: true,
  }))

  // Standard virtual store should be populated
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

  // Importer-level symlinks should NOT exist
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
  // No hoisted deps
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeFalsy()
  // No bin links
  expect(fs.existsSync(path.resolve('node_modules/.bin'))).toBeFalsy()
})

test('virtualStoreOnly suppresses hoisting even with explicit hoistPattern', async () => {
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  await install(manifest, testDefaults({
    virtualStoreOnly: true,
    hoistPattern: ['*'],
    publicHoistPattern: ['*'],
  }))

  // Virtual store should be populated
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+pkg-with-1-dep@100.0.0/node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()

  // No hoisted packages (despite hoistPattern: ['*'])
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeFalsy()
  // No importer-level symlinks
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
})
