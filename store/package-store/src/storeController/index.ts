import fs from 'node:fs'
import path from 'node:path'

import { type CafsLocker, createCafsStore, createPackageImporterAsync } from '@pnpm/create-cafs-store'
import type { Fetchers } from '@pnpm/fetcher-base'
import type { CustomFetcher } from '@pnpm/hooks.types'
import { createPackageRequester } from '@pnpm/package-requester'
import type { ResolveFunction } from '@pnpm/resolver-base'
import type { StoreIndex } from '@pnpm/store.index'
import type {
  ImportIndexedPackageAsync,
  StoreController,
} from '@pnpm/store-controller-types'
import { addFilesFromDir, importPackage, initStoreDir } from '@pnpm/worker'

import { prune } from './prune.js'

export { type CafsLocker }

export interface CreatePackageStoreOptions {
  cafsLocker?: CafsLocker
  engineStrict?: boolean
  force?: boolean
  nodeVersion?: string
  importPackage?: ImportIndexedPackageAsync
  pnpmVersion?: string
  ignoreFile?: (filename: string) => boolean
  cacheDir: string
  storeDir: string
  networkConcurrency?: number
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  verifyStoreIntegrity: boolean
  virtualStoreDirMaxLength: number
  strictStorePkgContentCheck?: boolean
  clearResolutionCache: () => void
  customFetchers?: CustomFetcher[]
  storeIndex: StoreIndex
}

export function createPackageStore (
  resolve: ResolveFunction,
  fetchers: Fetchers,
  initOpts: CreatePackageStoreOptions
): StoreController {
  const storeDir = initOpts.storeDir
  if (!fs.existsSync(path.join(storeDir, 'files'))) {
    initStoreDir(storeDir).catch()
  }
  const cafs = createCafsStore(storeDir, {
    cafsLocker: initOpts.cafsLocker,
    packageImportMethod: initOpts.packageImportMethod,
  })
  const packageRequester = createPackageRequester({
    force: initOpts.force,
    engineStrict: initOpts.engineStrict,
    nodeVersion: initOpts.nodeVersion,
    pnpmVersion: initOpts.pnpmVersion,
    resolve,
    fetchers,
    cafs,
    ignoreFile: initOpts.ignoreFile,
    networkConcurrency: initOpts.networkConcurrency,
    storeDir: initOpts.storeDir,
    verifyStoreIntegrity: initOpts.verifyStoreIntegrity,
    virtualStoreDirMaxLength: initOpts.virtualStoreDirMaxLength,
    strictStorePkgContentCheck: initOpts.strictStorePkgContentCheck,
    customFetchers: initOpts.customFetchers,
  })

  return {
    close: async () => {
      initOpts.storeIndex.flush()
    },
    fetchPackage: packageRequester.fetchPackageToStore,
    getFilesIndexFilePath: packageRequester.getFilesIndexFilePath,
    importPackage: initOpts.importPackage
      ? createPackageImporterAsync({ importIndexedPackage: initOpts.importPackage, storeDir: cafs.storeDir })
      : (targetDir, opts) => importPackage({
        ...opts,
        packageImportMethod: initOpts.packageImportMethod,
        storeDir: initOpts.storeDir,
        targetDir,
      }),
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir, storeIndex: initOpts.storeIndex }),
    requestPackage: packageRequester.requestPackage,
    upload,
    clearResolutionCache: initOpts.clearResolutionCache,
  }

  async function upload (builtPkgLocation: string, opts: { filesIndexFile: string, sideEffectsCacheKey: string }): Promise<void> {
    await addFilesFromDir({
      storeDir: cafs.storeDir,
      storeIndex: initOpts.storeIndex,
      dir: builtPkgLocation,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
      filesIndexFile: opts.filesIndexFile,
      pkg: {},
    })
  }
}
