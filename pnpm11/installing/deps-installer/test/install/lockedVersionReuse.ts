import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { mutateModules } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { preparePackages } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from '../utils/index.js'

test('a widened range moves to the higher version another importer already locks', async () => {
  const { install, readLockfile } = prepareWorkspace([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '1.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/foo': '1.2.0' } },
  ])
  await install()

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '^1.1.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/foo': '1.2.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(lockfile.importers['project-1' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/foo': { specifier: '^1.1.0', version: '1.2.0' },
  })
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['@pnpm.e2e/foo@1.2.0'])
})

test('a widened range the locked version still satisfies moves to the higher locked version', async () => {
  const { install, readLockfile } = prepareWorkspace([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '100.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/has-foo-100.1.0-dep-2': '1.0.0' } },
  ])
  await install()
  expect(Object.keys(readLockfile().packages ?? {}).filter(isFoo)).toStrictEqual([
    '@pnpm.e2e/foo@100.0.0',
    '@pnpm.e2e/foo@100.1.0',
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '>=100.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/has-foo-100.1.0-dep-2': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(lockfile.importers['project-1' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/foo': { specifier: '>=100.0.0', version: '100.1.0' },
  })
  expect(Object.keys(lockfile.packages ?? {}).filter(isFoo)).toStrictEqual(['@pnpm.e2e/foo@100.1.0'])
})

test('a range no locked version satisfies falls back to the resolver', async () => {
  const { install, readLockfile } = prepareWorkspace([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '1.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/foo': '1.2.0' } },
  ])
  await install()

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { '@pnpm.e2e/foo': '2.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/foo': '1.2.0' } },
  ])

  expect(requestedPackages).not.toStrictEqual([])
  expect(readLockfile().importers['project-1' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/foo': { specifier: '2.0.0', version: '2.0.0' },
  })
})

function isFoo (depPath: string): boolean {
  return depPath.startsWith('@pnpm.e2e/foo@')
}

interface WorkspaceProject {
  name: string
  dependencies: Record<string, string>
}

function prepareWorkspace (initial: WorkspaceProject[]) {
  preparePackages(initial.map(({ name }) => ({ location: name, package: { name } })))
  const mutation = initial.map(({ name }) => ({
    mutation: 'install' as const,
    rootDir: path.resolve(name) as ProjectRootDir,
  }))
  const install = async (projects: WorkspaceProject[] = initial): Promise<string[]> => {
    const options = testDefaults({
      allProjects: projects.map(({ name, dependencies }) => ({
        buildIndex: 0,
        manifest: { name, version: '1.0.0', dependencies },
        rootDir: path.resolve(name) as ProjectRootDir,
      })),
    })
    const requestedPackages = trackRequestedPackages(options.storeController)
    await mutateModules(mutation, options)
    return requestedPackages
  }
  return { install, readLockfile: () => readYamlFileSync<LockfileObject>(WANTED_LOCKFILE) }
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
