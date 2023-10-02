import { createCafsStore, createPackageImporterAsync, type CafsLocker } from '@pnpm/create-cafs-store'
import { type Fetchers } from '@pnpm/fetcher-base'
import { createPackageRequester } from '@pnpm/package-requester'
import { type ResolveFunction } from '@pnpm/resolver-base'
import {
  type ImportIndexedPackageAsync,
  type StoreController,
} from '@pnpm/store-controller-types'
import { addFilesFromDir, importPackage } from '@pnpm/worker'
import { prune } from './prune'

export { type CafsLocker }

export async function createPackageStore (
  resolve: ResolveFunction,
  fetchers: Fetchers,
  initOpts: {
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
      ? createPackageImporterAsync({ importIndexedPackage: initOpts.importPackage, cafsDir: cafs.cafsDir })
      : (targetDir, opts) => importPackage({
        ...opts,
        packageImportMethod: initOpts.packageImportMethod,
        storeDir: initOpts.storeDir,
        targetDir,
      }),
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir }),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function upload (builtPkgLocation: string, opts: { filesIndexFile: string, sideEffectsCacheKey: string }) {
    await addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: builtPkgLocation,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
      filesIndexFile: opts.filesIndexFile,
      pkg: {},
    })
  }
}
