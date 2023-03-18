/// <reference path="../../../__typings__/index.d.ts"/>
import { getContext } from '@pnpm/get-context'
import path from 'path'
import { type GetContextOptions } from '../src'

const DEFAULT_OPTIONS: GetContextOptions = {
  allProjects: [],
  extraBinPaths: [],
  force: false,
  forceSharedLockfile: false,
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
