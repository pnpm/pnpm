import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { addDependenciesToPackage, install, type MutatedProject, mutateModules, type PackageManifest } from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

function writeLegacyNodeEnginesPatch (nodeRange: string): Record<string, string> {
  const patchPath = path.resolve('for-legacy-node.patch')
  fs.writeFileSync(patchPath, `diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -2,6 +2,6 @@
   "name": "@pnpm.e2e/for-legacy-node",
   "version": "1.0.0",
   "engines": {
-    "node": "0.10"
+    "node": "${nodeRange}"
   }
 }
`)
  return { '@pnpm.e2e/for-legacy-node@1.0.0': patchPath }
}

function relaxNodeEngine (manifest: PackageManifest): PackageManifest {
  if (manifest.name === '@pnpm.e2e/for-legacy-node') {
    manifest.engines = { ...manifest.engines, node: '>=20' }
  }
  return manifest
}

function tightenNodeEngine (manifest: PackageManifest): PackageManifest {
  if (manifest.name === '@pnpm.e2e/for-legacy-node') {
    manifest.engines = { ...manifest.engines, node: '<0.10' }
  }
  return manifest
}

function tightenCompatibleNodeEngine (manifest: PackageManifest): PackageManifest {
  if (manifest.name === '@pnpm.e2e/foo') {
    manifest.engines = { ...manifest.engines, node: '<0.10' }
  }
  return manifest
}

test('fail if installed package does not support the current engine and engine-strict = true', async () => {
  const project = prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], testDefaults({}, {}, {}, {
      engineStrict: true,
    }))
  ).rejects.toThrow()
  project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHasNot('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
})

test('do not fail if installed package does not support the current engine and engine-strict = false', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-any-os'], testDefaults({
    engineStrict: false,
  }))

  project.has('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/not-compatible-with-any-os@1.0.0'].os).toStrictEqual(['this-os-does-not-exist'])
})

test('do not fail if installed package requires the node version that was passed in and engine-strict = true', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/for-legacy-node'], testDefaults({
    engineStrict: true,
    nodeVersion: '0.10.0',
  }))

  project.has('@pnpm.e2e/for-legacy-node')
  project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/for-legacy-node@1.0.0'].engines).toStrictEqual({ node: '0.10' })
})

test('readPackage hook relaxes engines for the engine-strict check', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/for-legacy-node'], testDefaults({
    hooks: {
      readPackage: [relaxNodeEngine],
    },
  }, {}, {}, { engineStrict: true }))

  project.has('@pnpm.e2e/for-legacy-node')
  project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/for-legacy-node@1.0.0'].engines).toStrictEqual({ node: '>=20' })
})

test('readPackage hook does not exempt a package whose engines are still unsatisfied', async () => {
  const project = prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/for-legacy-node'], testDefaults({
      hooks: {
        readPackage: [tightenNodeEngine],
      },
    }, {}, {}, { engineStrict: true }))
  ).rejects.toThrow()
  project.hasNot('@pnpm.e2e/for-legacy-node')
  project.storeHasNot('@pnpm.e2e/for-legacy-node', '1.0.0')
})

test('readPackage hook that tightens engines fails the engine-strict check', async () => {
  const project = prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['@pnpm.e2e/foo'], testDefaults({
      hooks: {
        readPackage: [tightenCompatibleNodeEngine],
      },
    }, {}, {}, { engineStrict: true }))
  ).rejects.toThrow()
  project.hasNot('@pnpm.e2e/foo')
})

test('readPackage hook is applied once per package request', async () => {
  const project = prepareEmpty()
  let calls = 0

  await addDependenciesToPackage({}, ['@pnpm.e2e/foo'], testDefaults({
    hooks: {
      readPackage: [(manifest: PackageManifest) => {
        if (manifest.name === '@pnpm.e2e/foo') calls++
        return manifest
      }],
    },
  }))

  project.has('@pnpm.e2e/foo')
  expect(calls).toBe(1)
})

test('a compatible package installs under engineStrict without a readPackage hook', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/foo'], testDefaults({}, {}, {}, { engineStrict: true }))

  project.has('@pnpm.e2e/foo')
  project.storeHas('@pnpm.e2e/foo', '100.1.0')

  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/foo@100.1.0']).not.toHaveProperty(['engines'])
})

test(`save cpu field to ${WANTED_LOCKFILE}`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/has-cpu-specified'], testDefaults())

  const lockfile = project.readLockfile()

  expect(
    lockfile.packages['@pnpm.e2e/has-cpu-specified@1.0.0'].cpu
  ).toStrictEqual(
    ['x64', 'ia32']
  )
})

test(`engines field is not added to ${WANTED_LOCKFILE} when "node": "*" is in "engines" field`, async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['jsonify@0.0.0'], testDefaults())

  const lockfile = project.readLockfile()

  expect(lockfile.packages['jsonify@0.0.0']).not.toHaveProperty(['engines'])
})

test('engine-strict checks the patched engines of a patched dependency', async () => {
  const project = prepareEmpty()
  const manifest = { dependencies: { '@pnpm.e2e/for-legacy-node': '1.0.0' } }
  const patchedDependencies = writeLegacyNodeEnginesPatch('*')

  await install(manifest, testDefaults({ engineStrict: true, patchedDependencies }, {}, {}, { engineStrict: true }))

  project.has('@pnpm.e2e/for-legacy-node')
  expect(JSON.parse(fs.readFileSync('node_modules/@pnpm.e2e/for-legacy-node/package.json', 'utf8')).engines)
    .toStrictEqual({ node: '*' })

  fs.rmSync('node_modules', { recursive: true })
  await install(manifest, testDefaults({ engineStrict: true, frozenLockfile: true, patchedDependencies }, {}, {}, { engineStrict: true }))

  project.has('@pnpm.e2e/for-legacy-node')
})

test('engine-strict skips an optional dependency whose patch makes its engines incompatible', async () => {
  prepareEmpty()
  const manifest = { optionalDependencies: { 'legacy-node': 'npm:@pnpm.e2e/for-legacy-node@1.0.0' } }
  const patchedDependencies = writeLegacyNodeEnginesPatch('99')

  await install(manifest, testDefaults({ engineStrict: true, patchedDependencies }, {}, {}, { engineStrict: true }))

  expect(() => fs.lstatSync('node_modules/legacy-node')).toThrow()

  fs.rmSync('node_modules', { recursive: true })
  await install(manifest, testDefaults({ engineStrict: true, frozenLockfile: true, patchedDependencies }, {}, {}, { engineStrict: true }))

  expect(() => fs.lstatSync('node_modules/legacy-node')).toThrow()
})

test('without engine-strict, a patched optional dependency with incompatible engines is skipped', async () => {
  prepareEmpty()
  const manifest = { optionalDependencies: { '@pnpm.e2e/for-legacy-node': '1.0.0' } }
  const patchedDependencies = writeLegacyNodeEnginesPatch('99')

  await install(manifest, testDefaults({ patchedDependencies }))

  expect(() => fs.lstatSync('node_modules/@pnpm.e2e/for-legacy-node')).toThrow()
})

test('engine-strict unlinks a skipped optional patched dependency from every workspace project', async () => {
  const optionalDependencies = { 'legacy-node': 'npm:@pnpm.e2e/for-legacy-node@1.0.0' }
  preparePackages([
    { location: 'project-1', package: { name: 'project-1', optionalDependencies } },
    { location: 'project-2', package: { name: 'project-2', optionalDependencies } },
  ])
  const patchedDependencies = writeLegacyNodeEnginesPatch('99')
  const rootDirs = ['project-1', 'project-2'].map((location) => path.resolve(location) as ProjectRootDir)
  const importers: MutatedProject[] = rootDirs.map((rootDir) => ({ mutation: 'install', rootDir }))
  const allProjects = rootDirs.map((rootDir, index) => ({
    buildIndex: 0,
    manifest: { name: `project-${index + 1}`, version: '1.0.0', optionalDependencies },
    rootDir,
  }))

  await mutateModules(importers, testDefaults({ allProjects, engineStrict: true, patchedDependencies }, {}, {}, { engineStrict: true }))

  for (const rootDir of rootDirs) {
    expect(() => fs.lstatSync(path.join(rootDir, 'node_modules/legacy-node'))).toThrow()
  }
})

test('engine-strict keeps the global virtual store slot of a skipped optional patched dependency', async () => {
  prepareEmpty()
  const globalVirtualStoreDir = path.resolve('links')
  const manifest = { optionalDependencies: { 'legacy-node': 'npm:@pnpm.e2e/for-legacy-node@1.0.0' } }
  const patchedDependencies = writeLegacyNodeEnginesPatch('99')

  await install(manifest, testDefaults({
    enableGlobalVirtualStore: true,
    engineStrict: true,
    patchedDependencies,
    virtualStoreDir: globalVirtualStoreDir,
  }, {}, {}, { engineStrict: true }))

  expect(() => fs.lstatSync('node_modules/legacy-node')).toThrow()
  const versionDir = path.join(globalVirtualStoreDir, '@pnpm.e2e/for-legacy-node/1.0.0')
  const slots = fs.readdirSync(versionDir)
  expect(slots).toHaveLength(1)
  expect(fs.existsSync(path.join(versionDir, slots[0], 'node_modules/@pnpm.e2e/for-legacy-node/package.json'))).toBe(true)
})
