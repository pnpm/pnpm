import path from 'node:path'

import { getContext } from '../src/index'
import type { GetContextOptions } from '@pnpm/types'

const DEFAULT_OPTIONS: GetContextOptions = {
  allProjects: [],
  autoInstallPeers: true,
  excludeLinksFromLockfile: false,
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
  expect(context.extraNodePaths).toEqual([
    path.join(context.virtualStoreDir, 'node_modules'),
  ])
})
