import path from 'node:path'
import url from 'node:url'

import { expect, test } from '@jest/globals'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { DepPath, ProjectId, ProjectRootDirRealPath } from '@pnpm/types'

import { createDeployFiles } from '../../src/deploy/createDeployFiles.js'

test('createDeployFiles keeps local tarball package names when rewriting file URLs', () => {
  const lockfileDir = path.resolve('workspace')
  const deployDir = path.join(lockfileDir, 'out')
  const tarball = path.join(lockfileDir, 'vendor/tar-pkg-1.0.0.tgz')
  const tarballUrl = url.pathToFileURL(tarball).toString()
  const inputDepPath = 'tar-pkg@file:vendor/tar-pkg-1.0.0.tgz' as DepPath
  const outputDepPath = `tar-pkg@${tarballUrl}` as DepPath
  const outputDepPathWithTarballFilename = `tar-pkg-1.0.0.tgz@${tarballUrl}` as DepPath
  const projectId = '.' as ProjectId
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
      injectWorkspacePackages: true,
    },
    importers: {
      [projectId]: {
        specifiers: {
          'tar-pkg': 'file:vendor/tar-pkg-1.0.0.tgz',
        },
        dependencies: {
          'tar-pkg': 'file:vendor/tar-pkg-1.0.0.tgz',
        },
      },
    },
    packages: {
      [inputDepPath]: {
        resolution: {
          integrity: 'sha512-test',
          tarball: 'file:vendor/tar-pkg-1.0.0.tgz',
        },
        version: '1.0.0',
      },
    },
  }

  const { lockfile: deployLockfile, manifest } = createDeployFiles({
    allProjects: [{
      rootDirRealPath: lockfileDir as ProjectRootDirRealPath,
      manifest: {
        name: 'app',
        version: '1.0.0',
      },
    }],
    deployDir,
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfile,
    lockfileDir,
    selectedProjectManifest: {
      name: 'app',
      version: '1.0.0',
    },
    projectId,
    rootProjectManifestDir: lockfileDir,
  })

  expect(manifest.dependencies).toStrictEqual({
    'tar-pkg': outputDepPath,
  })
  expect(deployLockfile.packages?.[outputDepPath]).toBeDefined()
  expect(deployLockfile.packages?.[outputDepPathWithTarballFilename]).toBeUndefined()
})

test('createDeployFiles drops optional edges of retained packages when optional dependencies are excluded', () => {
  const lockfileDir = path.resolve('workspace')
  const deployDir = path.join(lockfileDir, 'out')
  const projectId = '.' as ProjectId
  const keptDepPath = 'kept@1.0.0' as DepPath
  const optionalDepPath = 'opt@1.0.0' as DepPath
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      [projectId]: {
        specifiers: { kept: '1.0.0' },
        dependencies: { kept: '1.0.0' },
      },
    },
    packages: {
      [keptDepPath]: {
        resolution: { integrity: 'sha512-kept' },
        version: '1.0.0',
        optionalDependencies: { opt: '1.0.0' },
      },
      [optionalDepPath]: {
        resolution: { integrity: 'sha512-opt' },
        version: '1.0.0',
      },
    },
  }
  const commonOpts = {
    allProjects: [{
      rootDirRealPath: lockfileDir as ProjectRootDirRealPath,
      manifest: { name: 'app', version: '1.0.0' },
    }],
    deployDir,
    lockfile,
    lockfileDir,
    selectedProjectManifest: { name: 'app', version: '1.0.0' },
    projectId,
    rootProjectManifestDir: lockfileDir,
  }

  const withOptional = createDeployFiles({
    ...commonOpts,
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
  }).lockfile
  expect(withOptional.packages?.[keptDepPath].optionalDependencies).toStrictEqual({ opt: '1.0.0' })
  expect(withOptional.packages?.[optionalDepPath]).toBeDefined()

  const withoutOptional = createDeployFiles({
    ...commonOpts,
    include: { dependencies: true, devDependencies: true, optionalDependencies: false },
  }).lockfile
  expect(withoutOptional.packages?.[optionalDepPath]).toBeUndefined()
  expect(withoutOptional.packages?.[keptDepPath].optionalDependencies).toBeUndefined()
})

test('createDeployFiles drops excluded direct dependency groups from the importer and the manifest', () => {
  const lockfileDir = path.resolve('workspace')
  const deployDir = path.join(lockfileDir, 'out')
  const projectId = '.' as ProjectId
  const prodDepPath = 'prod@1.0.0' as DepPath
  const devDepPath = 'dev@1.0.0' as DepPath
  const optionalDepPath = 'opt@1.0.0' as DepPath
  const lockfile: LockfileObject = {
    lockfileVersion: '9.0',
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      [projectId]: {
        specifiers: { prod: '1.0.0', dev: '1.0.0', opt: '1.0.0' },
        dependencies: { prod: '1.0.0' },
        devDependencies: { dev: '1.0.0' },
        optionalDependencies: { opt: '1.0.0' },
      },
    },
    packages: {
      [prodDepPath]: { resolution: { integrity: 'sha512-prod' }, version: '1.0.0' },
      [devDepPath]: { resolution: { integrity: 'sha512-dev' }, version: '1.0.0' },
      [optionalDepPath]: { resolution: { integrity: 'sha512-opt' }, version: '1.0.0' },
    },
  }
  const commonOpts = {
    allProjects: [{
      rootDirRealPath: lockfileDir as ProjectRootDirRealPath,
      manifest: { name: 'app', version: '1.0.0' },
    }],
    deployDir,
    lockfile,
    lockfileDir,
    selectedProjectManifest: {
      name: 'app',
      version: '1.0.0',
      dependencies: { prod: '1.0.0' },
      devDependencies: { dev: '1.0.0' },
      optionalDependencies: { opt: '1.0.0' },
    },
    projectId,
    rootProjectManifestDir: lockfileDir,
  }

  const all = createDeployFiles({
    ...commonOpts,
    include: { dependencies: true, devDependencies: true, optionalDependencies: true },
  })
  expect(all.lockfile.importers[projectId].devDependencies).toStrictEqual({ dev: '1.0.0' })
  expect(all.lockfile.importers[projectId].optionalDependencies).toStrictEqual({ opt: '1.0.0' })
  expect(all.manifest.devDependencies).toStrictEqual({ dev: '1.0.0' })
  expect(all.manifest.optionalDependencies).toStrictEqual({ opt: '1.0.0' })

  const prodOnly = createDeployFiles({
    ...commonOpts,
    include: { dependencies: true, devDependencies: false, optionalDependencies: false },
  })
  expect(prodOnly.lockfile.importers[projectId].dependencies).toStrictEqual({ prod: '1.0.0' })
  expect(prodOnly.lockfile.importers[projectId].devDependencies).toStrictEqual({})
  expect(prodOnly.lockfile.importers[projectId].optionalDependencies).toStrictEqual({})
  expect(prodOnly.lockfile.importers[projectId].specifiers).toStrictEqual({ prod: '1.0.0' })
  expect(prodOnly.manifest.devDependencies).toStrictEqual({})
  expect(prodOnly.manifest.optionalDependencies).toStrictEqual({})
  expect(prodOnly.lockfile.packages?.[prodDepPath]).toBeDefined()
  expect(prodOnly.lockfile.packages?.[devDepPath]).toBeUndefined()
  expect(prodOnly.lockfile.packages?.[optionalDepPath]).toBeUndefined()
})
