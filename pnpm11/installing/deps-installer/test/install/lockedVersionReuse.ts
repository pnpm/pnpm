import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { mutateModules } from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { LockfileObject as LockfileTypesObject } from '@pnpm/lockfile.types'
import { preparePackages } from '@pnpm/prepare'
import type { StoreController } from '@pnpm/store.controller-types'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { tryComposeFastUpdates } from '../../src/install/tryComposeFastUpdates.js'
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

test('a higher version that exists only under a named registry falls back', async () => {
  // A registry-qualified key's semver only pins a version inside that named
  // registry, so it cannot become a plain importer reference.
  const subject = lockfileWithHigherVersionKeyedAs('@pnpm.e2e/foo@work:1.2.0' as DepPath)

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{
      id: '.' as ProjectId,
      manifest: { dependencies: { '@pnpm.e2e/foo': '^1.1.0' } } as ProjectManifest,
    }],
  })).toBe(false)
})

test('a higher version under a plain key is reused', async () => {
  const subject = lockfileWithHigherVersionKeyedAs('@pnpm.e2e/foo@1.2.0' as DepPath)

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{
      id: '.' as ProjectId,
      manifest: { dependencies: { '@pnpm.e2e/foo': '^1.1.0' } } as ProjectManifest,
    }],
  })).toBe(true)
  expect(subject.importers['.' as ProjectId].dependencies).toStrictEqual({
    '@pnpm.e2e/foo': '1.2.0',
  })
})

test('a moved range keeps a package whose peer suffix names the version it moves to', async () => {
  const subject = lockfileWithPeerDependentUnderQux()

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{
      id: '.' as ProjectId,
      manifest: { dependencies: { '@pnpm.e2e/foo': '^1.1.0', '@pnpm.e2e/qux': '^5.0.0' } } as ProjectManifest,
    }],
  })).toBe(true)
  expect(subject.importers['.' as ProjectId].dependencies!['@pnpm.e2e/foo']).toBe('1.2.0')
  expect(Object.keys(subject.packages ?? {}).filter(isFoo)).toStrictEqual(['@pnpm.e2e/foo@1.2.0'])
})

test('a moved range falls back when a peer suffix names the version it moves off', async () => {
  const subject = lockfileWithPeerDependentUnderQux()
  subject.importers['.' as ProjectId].specifiers['@pnpm.e2e/baz'] = '^4.0.0'
  subject.importers['.' as ProjectId].dependencies!['@pnpm.e2e/baz'] = '4.0.0(@pnpm.e2e/foo@1.0.0)'
  subject.packages!['@pnpm.e2e/baz@4.0.0(@pnpm.e2e/foo@1.0.0)' as DepPath] = {
    resolution: { integrity: 'sha512-baz-1' },
    dependencies: { '@pnpm.e2e/foo': '1.0.0' },
  }

  expect(await tryComposeFastUpdates(subject, {
    drift: { importers: true },
    workspacePackages: new Map(),
    resolutionPicksLowest: false,
    projects: [{
      id: '.' as ProjectId,
      manifest: {
        dependencies: {
          '@pnpm.e2e/foo': '^1.1.0',
          '@pnpm.e2e/qux': '^5.0.0',
          '@pnpm.e2e/baz': '^4.0.0',
        },
      } as ProjectManifest,
    }],
  })).toBe(false)
})

/**
 * The importer depends on `@pnpm.e2e/foo@1.0.0` directly, while
 * `@pnpm.e2e/baz` — reached through `@pnpm.e2e/qux` — resolves it as a peer
 * at the version `@pnpm.e2e/qux` provides, `1.2.0`.
 */
function lockfileWithPeerDependentUnderQux (): LockfileTypesObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '1.0.0', '@pnpm.e2e/qux': '^5.0.0' },
        dependencies: { '@pnpm.e2e/foo': '1.0.0', '@pnpm.e2e/qux': '5.0.0' },
      },
    },
    packages: {
      ['@pnpm.e2e/foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo-1' } },
      ['@pnpm.e2e/foo@1.2.0' as DepPath]: { resolution: { integrity: 'sha512-foo-2' } },
      ['@pnpm.e2e/qux@5.0.0' as DepPath]: {
        resolution: { integrity: 'sha512-qux' },
        dependencies: {
          '@pnpm.e2e/foo': '1.2.0',
          '@pnpm.e2e/baz': '4.0.0(@pnpm.e2e/foo@1.2.0)',
        },
      },
      ['@pnpm.e2e/baz@4.0.0(@pnpm.e2e/foo@1.2.0)' as DepPath]: {
        resolution: { integrity: 'sha512-baz-2' },
        dependencies: { '@pnpm.e2e/foo': '1.2.0' },
      },
    },
  }
}

function lockfileWithHigherVersionKeyedAs (higher: DepPath): LockfileTypesObject {
  return {
    lockfileVersion: '9.0',
    importers: {
      ['.' as ProjectId]: {
        specifiers: { '@pnpm.e2e/foo': '1.0.0' },
        dependencies: { '@pnpm.e2e/foo': '1.0.0' },
      },
    },
    packages: {
      ['@pnpm.e2e/foo@1.0.0' as DepPath]: { resolution: { integrity: 'sha512-foo-1' } },
      [higher]: { resolution: { integrity: 'sha512-foo-2' } },
    },
  }
}
