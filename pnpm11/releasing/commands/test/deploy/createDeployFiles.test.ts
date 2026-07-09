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
