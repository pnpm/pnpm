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

test('dropping a workspace package prunes its importer without resolving', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2' },
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  expect(Object.keys(readLockfile().importers)).toStrictEqual(['project-1'])
})

test('dropping a workspace package prunes what only it depended on', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1'])
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['is-positive@1.0.0'])
})

test('a dependency the dropped package shared with a survivor stays', async () => {
  const { install, readLockfile } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { 'is-positive': '1.0.0' } },
  ])

  const requestedPackages = await install([
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
  ])

  expect(requestedPackages).toStrictEqual([])
  const lockfile = readLockfile()
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1'])
  expect(Object.keys(lockfile.packages ?? {})).toStrictEqual(['is-positive@1.0.0'])
  expect(lockfile.importers['project-1' as ProjectId].dependencies).toStrictEqual({
    'is-positive': { specifier: '1.0.0', version: '1.0.0' },
  })
})

test('the fast path writes the lockfile a full resolution would', async () => {
  const { install, readLockfile } = prepareWorkspace()
  const initial: WorkspaceProject[] = [
    { name: 'project-1', dependencies: { 'is-positive': '1.0.0' } },
    { name: 'project-2', dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } },
  ]
  const surviving = initial.slice(0, 1)

  await install(initial)
  await install(surviving)
  const fastUpdated = readLockfile()

  await install(initial)
  await install(surviving, { forceFullResolution: true })

  expect(fastUpdated).toStrictEqual(readLockfile())
})

interface WorkspaceProject {
  name: string
  dependencies?: Record<string, string>
}

function prepareWorkspace () {
  const locations = ['project-1', 'project-2']
  preparePackages(locations.map((name) => ({ location: name, package: { name } })))
  const install = async (
    projects: WorkspaceProject[],
    extraOptions?: { forceFullResolution: boolean }
  ): Promise<string[]> => {
    const options = testDefaults({
      allProjects: projects.map(({ name, dependencies }) => ({
        buildIndex: 0,
        manifest: { name, version: '1.0.0', ...(dependencies ? { dependencies } : {}) },
        rootDir: path.resolve(name) as ProjectRootDir,
      })),
      pruneLockfileImporters: true,
      ...extraOptions,
    })
    const requestedPackages = trackRequestedPackages(options.storeController)
    await mutateModules(projects.map(({ name }) => ({
      mutation: 'install' as const,
      rootDir: path.resolve(name) as ProjectRootDir,
    })), options)
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

test('dropping a workspace package a survivor links to falls back to the resolver', async () => {
  const { install } = prepareWorkspace()
  await install([
    { name: 'project-1', dependencies: { 'project-2': 'workspace:1.0.0' } },
    { name: 'project-2' },
  ])

  await expect(install([
    { name: 'project-1', dependencies: { 'project-2': 'workspace:1.0.0' } },
  ])).rejects.toMatchObject({ code: 'ERR_PNPM_WORKSPACE_PKG_NOT_FOUND' })
})
