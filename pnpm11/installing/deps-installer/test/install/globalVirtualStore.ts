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

import { testDefaults } from '../utils/index.js'

const storeIndexes: StoreIndex[] = []
afterAll(() => {
  for (const si of storeIndexes) si.close()
})

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
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
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
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
  }
})

// https://github.com/pnpm/pnpm/issues/16642
test('an install that fetches a missing slot also repairs a slot missing a dependency link', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  })
  await install(manifest, opts)
  const parentVersionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0')
  const [hash] = fs.readdirSync(parentVersionDir)
  const childLink = path.join(parentVersionDir, hash, 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')

  fs.unlinkSync(childLink)
  rimrafSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/dep-of-pkg-with-1-dep'))
  await install(manifest, { ...opts, frozenLockfile: true })

  expect(fs.existsSync(path.join(childLink, 'package.json'))).toBeTruthy()
})

test('reinstall from warm global virtual store after deleting node_modules', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  })
  await install(manifest, opts)

  // Delete only node_modules, keep the global virtual store warm
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
  const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
  expect(files).toHaveLength(1)
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', files[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('fresh install into a second project reuses a warm global virtual store', async () => {
  const manifest = {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }
  preparePackages([
    { location: 'project-1', package: manifest },
    { location: 'project-2', package: manifest },
  ])

  const cwd = process.cwd()
  const project1Dir = path.resolve('project-1')
  const project2Dir = path.resolve('project-2')
  const globalVirtualStoreDir = path.resolve('links')
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    hoistPattern: ['*'],
  })

  try {
    process.chdir(project1Dir)
    await install(manifest, opts)
    fs.copyFileSync(path.join(project1Dir, 'pnpm-lock.yaml'), path.join(project2Dir, 'pnpm-lock.yaml'))

    const originalImportPackage = opts.storeController.importPackage
    let importCount = 0
    opts.storeController.importPackage = (async (targetDir, importOpts) => {
      const result = await originalImportPackage(targetDir, importOpts)
      if (result.importMethod != null) {
        importCount++
      }
      return result
    }) as typeof originalImportPackage

    process.chdir(project2Dir)
    await install(manifest, {
      ...opts,
      frozenLockfile: true,
    })

    expect(importCount).toBe(0)
    expect(fs.existsSync(path.join(project2Dir, 'node_modules/.pnpm/lock.yaml'))).toBeTruthy()
  } finally {
    process.chdir(cwd)
  }
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
    const files = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/peer-c/2.0.0'))
    expect(files).toHaveLength(1)
    expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/peer-c/2.0.0', files[0], 'node_modules/@pnpm.e2e/peer-c/package.json'))).toBeTruthy()
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
  const hashNoBuilds = fs.readdirSync(path.join(gvsDir1, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))[0]
  const hashWithBuilds = fs.readdirSync(path.join(gvsDir2, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))[0]

  // Hashes must differ: scenario 1 omits ENGINE_NAME, scenario 2 includes it
  // (because dep-of-pkg-with-1-dep is allowed to build)
  expect(hashNoBuilds).not.toBe(hashWithBuilds)

  // Both scenarios should still produce valid GVS layouts
  expect(fs.existsSync(path.join(gvsDir1, '@pnpm.e2e/pkg-with-1-dep/100.0.0', hashNoBuilds, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(gvsDir2, '@pnpm.e2e/pkg-with-1-dep/100.0.0', hashWithBuilds, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
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
  const hash1 = fs.readdirSync(path.join(gvsDir1, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))[0]
  const hash2 = fs.readdirSync(path.join(gvsDir2, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))[0]
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

  const hashBefore = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))[0]

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

  const hashAfter = fs.readdirSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0'))
    .find((hash) => hash !== hashBefore)

  // A new hash directory should have been created
  expect(hashAfter).toBeDefined()
  expect(hashAfter).not.toBe(hashBefore)

  // Verify the new GVS layout is valid
  expect(fs.existsSync(path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0', hashAfter!, 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()

  // Verify modules.yaml is updated with new allowBuilds
  const updatedState = rootModules.readModulesManifest()
  expect(updatedState?.allowBuilds).toEqual({ '@pnpm.e2e/dep-of-pkg-with-1-dep': true })
})

const BUILT_PKG = '@pnpm.e2e/pre-and-postinstall-scripts-example'

function readSlotPackageDirs (globalVirtualStoreDir: string, pkgName: string, version: string): string[] {
  const pkgVersionDir = path.join(globalVirtualStoreDir, pkgName, version)
  return fs.readdirSync(pkgVersionDir)
    .map((hash) => path.join(pkgVersionDir, hash, 'node_modules', pkgName))
}

/** The entries an install left in the scope directory of a slot's package, such as a staging directory. */
function readStagingLeftovers (pkgDir: string): string[] {
  return fs.readdirSync(path.dirname(pkgDir)).filter((entry) => entry.startsWith('.'))
}

test('GVS places a built package by renaming its staging directory into the slot', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      [BUILT_PKG]: '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    sideEffectsCacheWrite: true,
    allowBuilds: { [BUILT_PKG]: true },
  })
  await install(manifest, opts)

  const pkgDirs = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  expect(pkgDirs).toHaveLength(1)
  const [pkgInGvs] = pkgDirs
  expect(fs.existsSync(path.join(pkgInGvs, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-preinstall.js'))).toBeTruthy()
  expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])

  // The build output is cached from the staging directory.
  const filesIndexKey = storeIndexKey(getIntegrity(BUILT_PKG, '1.0.0'), `${BUILT_PKG}@1.0.0`)
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  const addedFiles = Array.from(filesIndex.sideEffects!.values()).flatMap((diff) => Array.from(diff.added?.keys() ?? []))
  expect(addedFiles).toContain('generated-by-postinstall.js')
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

  const pkgVersionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example/1.0.0')
  const hashBefore = fs.readdirSync(pkgVersionDir)
  expect(hashBefore).toHaveLength(1)

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
  const hashesAfter = fs.readdirSync(pkgVersionDir)
  const newHash = hashesAfter.find((hash) => hash !== hashBefore[0])
  expect(newHash).toBeDefined()
  expect(newHash).not.toBe(hashBefore[0])

  // Build artifacts in new hash directory
  const newPkgDir = path.join(pkgVersionDir, newHash!, 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example')
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(newPkgDir, 'generated-by-preinstall.js'))).toBeTruthy()

  // Build artifacts accessible via node_modules
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))).toBeTruthy()
})

test('GVS build failure leaves no package in the slot', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/failing-postinstall': '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/failing-postinstall': true },
  })
  // A repeat install builds the package again, since the failed build placed nothing.
  for (let attempt = 0; attempt < 2; attempt++) {
    // eslint-disable-next-line no-await-in-loop -- each install must see the store left by the previous one
    await expect(install(manifest, opts)).rejects.toThrow()

    // https://github.com/pnpm/pnpm/issues/15568
    const pkgDirs = readSlotPackageDirs(globalVirtualStoreDir, '@pnpm.e2e/failing-postinstall', '1.0.0')
    expect(pkgDirs).toHaveLength(1)
    expect(fs.existsSync(pkgDirs[0])).toBeFalsy()
    expect(readStagingLeftovers(pkgDirs[0])).toStrictEqual([])
  }
})

test('GVS removes an optional dependency whose build failed from its slot', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-failing-optional-dependency': '1.0.0',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { '@pnpm.e2e/pkg-with-failing-postinstall': true },
  })
  const failedVersionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-failing-postinstall/1.0.0')
  const parentVersionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-failing-optional-dependency/1.0.0')

  // A repeat install leaves the package removed.
  for (let attempt = 0; attempt < 2; attempt++) {
    // eslint-disable-next-line no-await-in-loop -- each install must see the store left by the previous one
    await install(manifest, opts)

    const hashes = fs.readdirSync(failedVersionDir)
    expect(hashes).toHaveLength(1)
    expect(fs.existsSync(path.join(failedVersionDir, hashes[0], 'node_modules/@pnpm.e2e/pkg-with-failing-postinstall'))).toBeFalsy()
    const parentHashes = fs.readdirSync(parentVersionDir)
    expect(parentHashes).toHaveLength(1)
    const parentModules = path.join(parentVersionDir, parentHashes[0], 'node_modules')
    expect(fs.existsSync(path.join(parentModules, '@pnpm.e2e/pkg-with-failing-optional-dependency/package.json'))).toBeTruthy()
    expect(fs.existsSync(path.join(parentModules, '@pnpm.e2e/pkg-with-failing-postinstall/package.json'))).toBeFalsy()
  }
})

test('GVS never rebuilds or overwrites a placed package', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      [BUILT_PKG]: '1.0.0',
    },
  }
  const opts = {
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { [BUILT_PKG]: true },
  }
  await install(manifest, testDefaults(opts))

  const [pkgInGvs] = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  // Changes that a rebuild or a re-import of the slot would undo.
  fs.unlinkSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))
  fs.unlinkSync(path.join(pkgInGvs, 'README.md'))

  for (const extraOpts of [{ frozenLockfile: true }, { force: true }]) {
    rimrafSync('node_modules')
    // eslint-disable-next-line no-await-in-loop -- each install must see the store left by the previous one
    await install(manifest, testDefaults({ ...opts, ...extraOpts }))

    expect(fs.realpathSync(path.resolve('node_modules', BUILT_PKG))).toBe(fs.realpathSync(pkgInGvs))
    expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeFalsy()
    expect(fs.existsSync(path.join(pkgInGvs, 'README.md'))).toBeFalsy()
    expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])
  }
})

test('GVS install with ignoreScripts gets a different slot than a building install', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const patchPath = path.resolve('pre-and-postinstall-scripts-example.patch')
  fs.writeFileSync(patchPath, `diff --git a/patched.txt b/patched.txt
new file mode 100644
--- /dev/null
+++ b/patched.txt
@@ -0,0 +1 @@
+applied
`)
  const manifest = {
    dependencies: {
      [BUILT_PKG]: '1.0.0',
    },
  }
  const opts = {
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { [BUILT_PKG]: true },
    patchedDependencies: { [`${BUILT_PKG}@1.0.0`]: patchPath },
  }
  await install(manifest, testDefaults({ ...opts, ignoreScripts: true }))
  const [pristinePkg] = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  expect(fs.existsSync(path.join(pristinePkg, 'patched.txt'))).toBeTruthy()
  expect(fs.existsSync(path.join(pristinePkg, 'generated-by-postinstall.js'))).toBeFalsy()

  rimrafSync('node_modules')
  await install(manifest, testDefaults({ ...opts, frozenLockfile: true }))

  const builtPkg = fs.realpathSync(path.resolve('node_modules', BUILT_PKG))
  expect(builtPkg).not.toBe(fs.realpathSync(pristinePkg))
  expect(fs.existsSync(path.join(builtPkg, 'patched.txt'))).toBeTruthy()
  expect(fs.existsSync(path.join(builtPkg, 'generated-by-postinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(pristinePkg, 'generated-by-postinstall.js'))).toBeFalsy()
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

  const pkgDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pre-and-postinstall-scripts-example/1.0.0')
  const hashes = fs.readdirSync(pkgDir)
  expect(hashes).toHaveLength(1)
  const hashDir = path.join(pkgDir, hashes[0])
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
  const hashesAfter = fs.readdirSync(pkgDir)
  expect(hashesAfter).toHaveLength(1)
  expect(fs.existsSync(path.join(pkgDir, hashesAfter[0], 'node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js'))).toBeTruthy()
})

test('GVS imports a cached build into the slot in place', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      [BUILT_PKG]: '1.0.0',
    },
  }
  const installOpts = {
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    sideEffectsCacheRead: true,
    sideEffectsCacheWrite: true,
    verifyStoreIntegrity: false,
    allowBuilds: { [BUILT_PKG]: true },
  }
  const opts = testDefaults(installOpts)
  await install(manifest, opts)

  // Drop a file from the cached build, so a package imported from the cache lacks it.
  const filesIndexKey = storeIndexKey(getIntegrity(BUILT_PKG, '1.0.0'), `${BUILT_PKG}@1.0.0`)
  const storeIndex = new StoreIndex(opts.storeDir)
  storeIndexes.push(storeIndex)
  const filesIndex = storeIndex.get(filesIndexKey) as PackageFilesIndex
  for (const diff of filesIndex.sideEffects!.values()) {
    diff.added?.delete('generated-by-postinstall.js')
  }
  storeIndex.set(filesIndexKey, filesIndex)

  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)
  await install(manifest, testDefaults({ ...installOpts, frozenLockfile: true, storeDir: opts.storeDir }))

  const [pkgInGvs] = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-preinstall.js'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeFalsy()
  expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])
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

// The lockfile records no version for a directory snapshot, so the headless
// install has none to put in the slot path.
// See https://github.com/pnpm/pnpm/issues/13335.
test('local directory dependency works with global virtual store', async () => {
  const project = prepareEmpty()
  fs.mkdirSync('dep')
  fs.writeFileSync('dep/package.json', JSON.stringify({ name: 'dep', version: '1.0.0' }), 'utf8')

  const globalVirtualStoreDir = path.resolve('links')
  const manifest = {
    dependencies: {
      dep: 'file:dep',
    },
  }
  const opts = testDefaults({
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  })
  await install(manifest, opts)

  project.has('dep')
  const slotBeforeReinstall = fs.realpathSync(path.resolve('node_modules/dep'))
  expect(slotBeforeReinstall.startsWith(globalVirtualStoreDir)).toBeTruthy()

  rimrafSync('node_modules')
  await install(manifest, { ...opts, frozenLockfile: true })

  project.has('dep')
  // The resolver reads the version off the manifest and the headless install
  // reads it off the lockfile, where there is none — both have to land on the
  // same slot or the reinstall would relocate the package.
  expect(fs.realpathSync(path.resolve('node_modules/dep'))).toBe(slotBeforeReinstall)
})

test('GVS rebuilds a built local directory dependency whose files changed', async () => {
  prepareEmpty()
  fs.mkdirSync('dep')
  fs.writeFileSync('dep/package.json', JSON.stringify({
    name: 'dep',
    version: '1.0.0',
    scripts: { postinstall: 'node -e "require(\'fs\').copyFileSync(\'data.txt\', \'built.txt\')"' },
  }), 'utf8')
  fs.writeFileSync('dep/data.txt', 'v1')
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = { dependencies: { dep: 'file:dep' } }
  const installOpts = {
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    dangerouslyAllowAllBuilds: true,
  }
  const opts = testDefaults(installOpts)
  await install(manifest, opts)
  const pkgInGvs = fs.realpathSync(path.resolve('node_modules/dep'))
  expect(fs.readFileSync(path.join(pkgInGvs, 'built.txt'), 'utf8')).toBe('v1')

  fs.writeFileSync('dep/data.txt', 'v2')
  await install(manifest, testDefaults({ ...installOpts, storeDir: opts.storeDir }))

  expect(fs.realpathSync(path.resolve('node_modules/dep'))).toBe(pkgInGvs)
  expect(fs.readFileSync(path.join(pkgInGvs, 'data.txt'), 'utf8')).toBe('v2')
  expect(fs.readFileSync(path.join(pkgInGvs, 'built.txt'), 'utf8')).toBe('v2')
  expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])
})

test('two projects with a same-named local directory dependency get separate global virtual store slots', async () => {
  preparePackages([
    { location: 'project-1', package: { name: 'project-1', version: '1.0.0' } },
    { location: 'project-2', package: { name: 'project-2', version: '1.0.0' } },
  ])
  const globalVirtualStoreDir = path.resolve('links')

  // The second install has to see what the first one left in the shared store,
  // so the two run one after the other.
  const slotOfProject1 = await installLocalDirectoryDependency('project-1', globalVirtualStoreDir)
  const slotOfProject2 = await installLocalDirectoryDependency('project-2', globalVirtualStoreDir)

  // Both resolve to `dep@file:dep` with no recorded version — only the project
  // they belong to tells the two slots apart.
  expect(slotOfProject1).not.toBe(slotOfProject2)
  expect(fs.readFileSync(path.resolve('project-1/node_modules/dep/index.js'), 'utf8')).toBe("module.exports = 'project-1'")
  expect(fs.readFileSync(path.resolve('project-2/node_modules/dep/index.js'), 'utf8')).toBe("module.exports = 'project-2'")
})

/** Installs a `./dep` directory dependency into `project` and returns its slot. */
async function installLocalDirectoryDependency (project: string, globalVirtualStoreDir: string): Promise<string> {
  fs.mkdirSync(path.resolve(project, 'dep'))
  fs.writeFileSync(path.resolve(project, 'dep/package.json'), JSON.stringify({ name: 'dep', version: '1.0.0' }), 'utf8')
  fs.writeFileSync(path.resolve(project, 'dep/index.js'), `module.exports = '${project}'`, 'utf8')
  await install({ dependencies: { dep: 'file:dep' } }, testDefaults({
    dir: path.resolve(project),
    lockfileDir: path.resolve(project),
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
  }))
  return fs.realpathSync(path.resolve(project, 'node_modules/dep'))
}

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
  const pkgDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0')
  expect(fs.existsSync(pkgDir)).toBeTruthy()
  const hashes = fs.readdirSync(pkgDir)
  expect(hashes).toHaveLength(1)
  expect(fs.existsSync(path.join(pkgDir, hashes[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
})

test('virtualStoreOnly with enableModulesDir=false places packages that need a build in unbuilt slots', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('gvs-no-modules')
  const manifest = { dependencies: { [BUILT_PKG]: '1.0.0' } }
  const installOpts = {
    enableGlobalVirtualStore: true,
    virtualStoreDir: globalVirtualStoreDir,
    fastUnpack: false,
    allowBuilds: { [BUILT_PKG]: true },
  }
  await install(manifest, testDefaults(installOpts))
  const [builtPkg] = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  rimrafSync('node_modules')
  rimrafSync(globalVirtualStoreDir)

  await install(manifest, testDefaults({ ...installOpts, virtualStoreOnly: true, enableModulesDir: false, frozenLockfile: true }))

  const [pkgInGvs] = readSlotPackageDirs(globalVirtualStoreDir, BUILT_PKG, '1.0.0')
  expect(pkgInGvs).not.toBe(builtPkg)
  expect(fs.existsSync(path.join(pkgInGvs, 'package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgInGvs, 'generated-by-postinstall.js'))).toBeFalsy()
  expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])
})

test('GVS runs a placed package\'s build again when its effects are outside the package', async () => {
  prepareEmpty()
  const log = path.resolve('outside-log')
  fs.writeFileSync(log, '')
  process.env.PNPM_E2E_OUTSIDE_LOG = log
  try {
    const globalVirtualStoreDir = path.resolve('links')
    const pkgName = '@pnpm.e2e/postinstall-writes-outside-package'
    const manifest = { dependencies: { [pkgName]: '1.1.0' } }
    const installOpts = {
      enableGlobalVirtualStore: true,
      virtualStoreDir: globalVirtualStoreDir,
      fastUnpack: false,
      sideEffectsCacheRead: true,
      sideEffectsCacheWrite: true,
      allowBuilds: { [pkgName]: true },
    }
    const opts = testDefaults(installOpts)
    await install(manifest, opts)
    expect(fs.readFileSync(log, 'utf8')).toBe('x')
    const [pkgInGvs] = readSlotPackageDirs(globalVirtualStoreDir, pkgName, '1.1.0')
    const placedFiles = fs.readdirSync(pkgInGvs).sort()

    rimrafSync('node_modules')
    await install(manifest, testDefaults({ ...installOpts, frozenLockfile: true, storeDir: opts.storeDir }))

    expect(fs.readFileSync(log, 'utf8')).toBe('xx')
    expect(fs.realpathSync(path.resolve('node_modules', pkgName))).toBe(pkgInGvs)
    expect(fs.readdirSync(pkgInGvs).sort()).toStrictEqual(placedFiles)
    expect(readStagingLeftovers(pkgInGvs)).toStrictEqual([])
  } finally {
    delete process.env.PNPM_E2E_OUTSIDE_LOG
  }
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
  const pkgDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0')
  expect(fs.existsSync(pkgDir)).toBeTruthy()
  const hashes = fs.readdirSync(pkgDir)
  expect(hashes).toHaveLength(1)
  expect(fs.existsSync(path.join(pkgDir, hashes[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  expect(fs.existsSync(path.join(pkgDir, hashes[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

  // Importer-level links should NOT exist
  expect(fs.existsSync(path.resolve('node_modules/@pnpm.e2e/pkg-with-1-dep'))).toBeFalsy()
  // No hoisted deps
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep'))).toBeFalsy()
  // No bin links
  expect(fs.existsSync(path.resolve('node_modules/.bin'))).toBeFalsy()
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
  const pkgDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/pkg-with-1-dep/100.0.0')
  expect(fs.existsSync(pkgDir)).toBeTruthy()
  const hashes = fs.readdirSync(pkgDir)
  expect(hashes).toHaveLength(1)
  expect(fs.existsSync(path.join(pkgDir, hashes[0], 'node_modules/@pnpm.e2e/pkg-with-1-dep/package.json'))).toBeTruthy()
  // Transitive dependency should also be in GVS
  expect(fs.existsSync(path.join(pkgDir, hashes[0], 'node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json'))).toBeTruthy()

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
