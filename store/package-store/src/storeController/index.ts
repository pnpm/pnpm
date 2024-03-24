import type {
  Fetchers,
  StoreController,
  ResolveFunction,
  ImportIndexedPackageAsync,
} from '@pnpm/types'
import {
  type CafsLocker,
  createCafsStore,
  createPackageImporterAsync,
} from '@pnpm/create-cafs-store'
import { addFilesFromDir, importPackage } from '@pnpm/worker'
import { createPackageRequester } from '@pnpm/package-requester'

import { prune } from './prune.js'

export async function createPackageStore(
  resolve: ResolveFunction,
  fetchers: Fetchers,
  initOpts: {
    cafsLocker?: CafsLocker | undefined
    engineStrict?: boolean | undefined
    force?: boolean | undefined
    nodeVersion?: string | undefined
    importPackage?: ImportIndexedPackageAsync | undefined
    pnpmVersion?: string | undefined
    ignoreFile?: ((filename: string) => boolean) | undefined
    cacheDir?: string | undefined
    storeDir: string
    networkConcurrency?: number | undefined
    packageImportMethod?:
      | 'auto'
      | 'hardlink'
      | 'copy'
      | 'clone'
      | 'clone-or-copy'
      | undefined
    verifyStoreIntegrity: boolean
  }
): Promise<StoreController> {
  const storeDir = initOpts.storeDir
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
  })

  return {
    close: async () => {}, // eslint-disable-line:no-empty
    fetchPackage: packageRequester.fetchPackageToStore,
    getFilesIndexFilePath: packageRequester.getFilesIndexFilePath,
    importPackage: initOpts.importPackage
      ? createPackageImporterAsync({
        importIndexedPackage: initOpts.importPackage,
        cafsDir: cafs.cafsDir,
      })
      : (targetDir, opts) => {
        return importPackage({
          ...opts,
          packageImportMethod: initOpts.packageImportMethod,
          storeDir: initOpts.storeDir,
          targetDir,
        });
      },
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir ?? '' }),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function upload(
    builtPkgLocation: string,
    opts: { filesIndexFile: string; sideEffectsCacheKey: string }
  ) {
    await addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: builtPkgLocation,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
      filesIndexFile: opts.filesIndexFile,
      pkg: {},
    })
  }
}
