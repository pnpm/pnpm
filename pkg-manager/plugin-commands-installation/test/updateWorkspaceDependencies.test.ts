import { type PnpmError } from '@pnpm/error'
import { type ProjectRootDir } from '@pnpm/types'
import {
  createWorkspaceSpecs,
  updateToWorkspacePackagesFromManifest,
} from '../lib/updateWorkspaceDependencies'

const INCLUDE_ALL = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
}

const WORKSPACE_PACKAGES = new Map([
  ['bar', new Map([
    ['100.0.0', {
      rootDir: '' as ProjectRootDir,
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    }],
  ])],
  ['foo', new Map([
    ['100.0.0', {
      rootDir: '' as ProjectRootDir,
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    }],
  ])],
  ['qar', new Map([
    ['100.0.0', {
      rootDir: '' as ProjectRootDir,
      manifest: {
        name: 'foo',
        version: '100.0.0',
      },
    }],
  ])],
])

test('updateToWorkspacePackagesFromManifest()', () => {
  const manifest = {
    dependencies: {
      alpha: '1.0.0',
      foo: '1.0.0',
    },
    devDependencies: {
      bar: '1.0.0',
      betta: '1.0.0',
    },
    optionalDependencies: {
      hamma: '1.0.0', // cspell:disable-line
      qar: '1.0.0',
    },
  }
  expect(updateToWorkspacePackagesFromManifest(
    manifest,
    INCLUDE_ALL,
    WORKSPACE_PACKAGES
  )).toStrictEqual(['bar@workspace:*', 'foo@workspace:*', 'qar@workspace:*'])
  expect(updateToWorkspacePackagesFromManifest(
    manifest,
    {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: false,
    },
    WORKSPACE_PACKAGES
  )).toStrictEqual(['foo@workspace:*'])
})

test('createWorkspaceSpecs', () => {
  expect(createWorkspaceSpecs(['bar', 'foo@2', 'qar@workspace:3'], WORKSPACE_PACKAGES)).toStrictEqual(['bar@workspace:>=0.0.0', 'foo@workspace:2', 'qar@workspace:3'])
  let err!: PnpmError
  try {
    createWorkspaceSpecs(['express'], WORKSPACE_PACKAGES)
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_WORKSPACE_PACKAGE_NOT_FOUND')
  expect(err.message).toBe('"express" not found in the workspace')
})
