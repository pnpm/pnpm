import { expect, test } from '@jest/globals'
import { mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

test('a removal from a time-based lockfile skips resolution and writes what a resolution would', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/bravo': '1.0.0',
      '@pnpm.e2e/romeo': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ resolutionMode: 'time-based' }))
  const installed = project.readLockfile()
  expect(Object.keys(installed.time).sort()).toStrictEqual([
    '@pnpm.e2e/bravo@1.0.0',
    '@pnpm.e2e/romeo@1.0.0',
  ])

  const options = testDefaults({ resolutionMode: 'time-based' })
  const requestedPackages = trackRequestedPackages(options.storeController)
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/romeo'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const fastUpdated = project.readLockfile()
  expect(Object.keys(fastUpdated.time)).toStrictEqual(['@pnpm.e2e/bravo@1.0.0'])

  // The same removal, resolved rather than rewritten: the two lockfiles have
  // to agree, or the fast path writes something the resolver never would.
  const baseline = await resolveTheSameRemoval()
  expect(fastUpdated).toStrictEqual(baseline)
})

async function resolveTheSameRemoval (): Promise<unknown> {
  const previousCwd = process.cwd()
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/bravo': '1.0.0',
      '@pnpm.e2e/romeo': '1.0.0',
    },
  }
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ resolutionMode: 'time-based' }))
  await mutateModulesInSingleProject({
    dependencyNames: ['@pnpm.e2e/romeo'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ resolutionMode: 'time-based', forceFullResolution: true }))
  const baseline = project.readLockfile()
  process.chdir(previousCwd)
  return baseline
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
