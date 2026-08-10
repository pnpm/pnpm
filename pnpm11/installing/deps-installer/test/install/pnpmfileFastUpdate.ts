import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { requireHooks } from '@pnpm/hooks.pnpmfile'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { PackageManifest, ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test('a remove with an unchanged pnpmfile skips resolution', async () => {
  const project = prepareEmpty()
  // The hook pins a transitive dependency, so its effect is visible in the
  // lockfile and must survive the fast update.
  function readPackage (manifest: PackageManifest) {
    if (manifest.name === '@pnpm.e2e/pkg-with-1-dep') {
      manifest.dependencies!['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hooks: { readPackage: [readPackage], calculatePnpmfileChecksum } }))

  const options = testDefaults({ hooks: { readPackage: [readPackage], calculatePnpmfileChecksum } })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages).some((depPath) => depPath.startsWith('is-positive@'))).toBe(false)
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.0.0'].dependencies).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
  })
})

test('a remove keeps the specifiers a project-rewriting pnpmfile recorded', async () => {
  const project = prepareEmpty()
  // The hook pins a direct dependency of the project itself, so the recorded
  // specifier differs from the raw manifest. The fast update must compare
  // hooked manifests — absorbing the raw specifier would commit a lockfile a
  // full resolution would never write.
  function readPackage (manifest: PackageManifest) {
    if (manifest.dependencies?.['is-positive'] != null) {
      manifest.dependencies['is-positive'] = '1.0.0'
    }
    return manifest
  }
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '^1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hooks: { readPackage: [readPackage], calculatePnpmfileChecksum } }))
  expect(project.readLockfile().importers['.'].dependencies!['is-positive'].specifier).toBe('1.0.0')

  const options = testDefaults({ hooks: { readPackage: [readPackage], calculatePnpmfileChecksum } })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/pkg-with-1-dep'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies!['is-positive'].specifier).toBe('1.0.0')
  expect(Object.keys(lockfile.packages).some((depPath) => depPath.startsWith('@pnpm.e2e/pkg-with-1-dep@'))).toBe(false)
})

test('a programmatic hook without a pnpmfile checksum still resolves', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '1.0.0',
    },
  }
  const readPackage = (pkg: PackageManifest) => pkg
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hooks: { readPackage: [readPackage] } }))

  const options = testDefaults({ hooks: { readPackage: [readPackage] } })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).not.toStrictEqual([])
})

test('a hook from the checksum-excluded global pnpmfile still resolves', async () => {
  prepareEmpty()
  fs.writeFileSync('global-pnpmfile.cjs', 'module.exports = { hooks: { readPackage: (pkg) => pkg } }')
  const loadHooks = async () => (await requireHooks(process.cwd(), {
    globalPnpmfile: path.resolve('global-pnpmfile.cjs'),
  })).hooks
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
      'is-positive': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ hooks: await loadHooks() }))

  const options = testDefaults({ hooks: await loadHooks() })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['is-positive'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).not.toStrictEqual([])
})

/**
 * Marks the hooks as coming from a pnpmfile whose content is stable across
 * the installs of one test, the way `requireHooks` tracks real pnpmfiles.
 */
async function calculatePnpmfileChecksum (): Promise<string> {
  return 'test-pnpmfile-checksum'
}

function trackRequestedPackages (storeController: StoreController): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}
