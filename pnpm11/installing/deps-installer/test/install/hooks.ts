import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  type PackageManifest,
} from '@pnpm/installing.deps-installer'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from '../utils/index.js'

test('readPackage, afterAllResolved hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  const afterAllResolved = jest.fn((lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage, afterAllResolved async hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  async function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  const afterAllResolved = jest.fn(async (lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage rewrites the specifier of the project own dependency', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    if (manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep'] != null) {
      manifest.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
    },
  }, testDefaults({
    hooks: {
      readPackage: [readPackageHook],
    },
  }))

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toStrictEqual({
    specifier: '100.0.0',
    version: '100.0.0',
  })
})

test('readPackage rewrites the specifier of a workspace member own dependency', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    if (manifest.dependencies?.['@pnpm.e2e/pkg-with-1-dep'] != null) {
      manifest.dependencies['@pnpm.e2e/pkg-with-1-dep'] = '100.0.0'
    }
    return manifest
  }
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          '@pnpm.e2e/pkg-with-1-dep': '^100.0.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ]
  const mutation = [{ mutation: 'install' as const, rootDir: path.resolve('project-1') as ProjectRootDir }]

  await mutateModules(mutation, testDefaults({
    allProjects,
    hooks: { readPackage: [readPackageHook] },
  }))

  const recorded = {
    specifier: '100.0.0',
    version: '100.0.0',
  }
  const readMemberEntry = (): unknown => {
    const lockfile = readYamlFileSync<LockfileObject>(WANTED_LOCKFILE)
    return lockfile.importers['project-1' as ProjectId].dependencies?.['@pnpm.e2e/pkg-with-1-dep']
  }
  expect(readMemberEntry()).toStrictEqual(recorded)

  // The repeat install must compare against the hooked manifest too, or
  // the raw range reads as drift and the entry is rewritten.
  await mutateModules(mutation, testDefaults({
    allProjects,
    hooks: { readPackage: [readPackageHook] },
  }))

  expect(readMemberEntry()).toStrictEqual(recorded)
})

test('readPackage hooks array', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook1 (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '50.0.0'
        break
    }
    return manifest
  }

  function readPackageHook2 (manifest: PackageManifest) {
    switch (manifest.name) {
      case '@pnpm.e2e/pkg-with-1-dep':
        if (manifest.dependencies == null) {
          throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
        }
        manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return manifest
  }

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      readPackage: [readPackageHook1, readPackageHook2],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})
