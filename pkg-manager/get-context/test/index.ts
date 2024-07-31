/// <reference path="../../../__typings__/index.d.ts"/>
import { getContext, arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { type ProjectRootDir } from '@pnpm/types'
import path from 'path'
import { type GetContextOptions } from '../src'

const DEFAULT_OPTIONS: GetContextOptions = {
  allProjects: [],
  autoInstallPeers: true,
  excludeLinksFromLockfile: false,
  extraBinPaths: [],
  force: false,
  lockfileDir: path.join(__dirname, 'lockfile'),
  nodeLinker: 'isolated',
  hoistPattern: ['*'],
  registries: { default: '' },
  useLockfile: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  storeDir: path.join(__dirname, 'store'),
  virtualStoreDirMaxLength: 120,
  peersSuffixMaxLength: 1000,
}

test('getContext - extendNodePath false', async () => {
  const context = await getContext({
    ...DEFAULT_OPTIONS,
    extendNodePath: false,
  })
  expect(context.extraNodePaths).toEqual([])
})

test('getContext - extendNodePath true', async () => {
  const context = await getContext({
    ...DEFAULT_OPTIONS,
    extendNodePath: true,
  })
  expect(context.extraNodePaths).toEqual([path.join(context.virtualStoreDir, 'node_modules')])
})

// This is supported for compatibility with Yarn's implementation
// see https://github.com/pnpm/pnpm/issues/2648
test('arrayOfWorkspacePackagesToMap() treats private packages with no version as packages with 0.0.0 version', () => {
  const privateProject = {
    rootDir: process.cwd() as ProjectRootDir,
    manifest: {
      name: 'private-pkg',
    },
  }
  expect(arrayOfWorkspacePackagesToMap([privateProject])).toStrictEqual(new Map([
    ['private-pkg', new Map([
      ['0.0.0', privateProject],
    ])],
  ]))
})
