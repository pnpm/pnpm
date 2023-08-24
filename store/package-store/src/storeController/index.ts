import {
  type PackageFilesIndex,
} from '@pnpm/store.cafs'
import { createCafsStore, createPackageImporterAsync, type CafsLocker } from '@pnpm/create-cafs-store'
import { type Fetchers } from '@pnpm/fetcher-base'
import { PnpmError } from '@pnpm/error'
import { createPackageRequester } from '@pnpm/package-requester'
import { type ResolveFunction } from '@pnpm/resolver-base'
import {
  type ImportIndexedPackageAsync,
  type PackageFileInfo,
  type StoreController,
} from '@pnpm/store-controller-types'
import { workerPool as pool } from '@pnpm/fetching.tarball-worker'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
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
    relinkLocalDirDeps?: boolean
    verifyStoreIntegrity: boolean
  }
): Promise<StoreController> {
  pool.reset()
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
    relinkLocalDirDeps: initOpts.relinkLocalDirDeps,
  })

  return {
    close: async () => {
      // @ts-expect-error
      global.finishWorkers?.()
    },
    fetchPackage: packageRequester.fetchPackageToStore,
    getFilesIndexFilePath: packageRequester.getFilesIndexFilePath,
    importPackage: initOpts.importPackage
      ? createPackageImporterAsync({ importIndexedPackage: initOpts.importPackage, cafsDir: cafs.cafsDir })
      : async (targetDir, opts) => {
        const localWorker = await pool.checkoutWorkerAsync(true)
        return new Promise<{ isBuilt: boolean, importMethod: string | undefined }>((resolve, reject) => {
          localWorker.once('message', ({ status, error, value }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
            pool.checkinWorker(localWorker)
            if (status === 'error') {
              reject(new PnpmError('LINKING_FAILED', error as string))
              return
            }
            resolve(value)
          })
          localWorker.postMessage({
            type: 'link',
            filesResponse: opts.filesResponse,
            packageImportMethod: initOpts.packageImportMethod,
            sideEffectsCacheKey: opts.sideEffectsCacheKey,
            storeDir: initOpts.storeDir,
            targetDir,
            requiresBuild: opts.requiresBuild,
            force: opts.force,
            keepModulesDir: opts.keepModulesDir,
          })
        })
      },
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir }),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function upload (builtPkgLocation: string, opts: { filesIndexFile: string, sideEffectsCacheKey: string }) {
    const sideEffectsIndex = await cafs.addFilesFromDir(builtPkgLocation)
    // TODO: move this to a function
    // This is duplicated in @pnpm/package-requester
    const integrity: Record<string, PackageFileInfo> = {}
    await Promise.all(
      Object.entries(sideEffectsIndex)
        .map(async ([filename, { writeResult, mode, size }]) => {
          const {
            checkedAt,
            integrity: fileIntegrity,
          } = await writeResult
          integrity[filename] = {
            checkedAt,
            integrity: fileIntegrity.toString(), // TODO: use the raw Integrity object
            mode,
            size,
          }
        })
    )
    let filesIndex!: PackageFilesIndex
    try {
      filesIndex = await loadJsonFile<PackageFilesIndex>(opts.filesIndexFile)
    } catch (err: any) { // eslint-disable-line
      filesIndex = { files: integrity }
    }
    filesIndex.sideEffects = filesIndex.sideEffects ?? {}
    filesIndex.sideEffects[opts.sideEffectsCacheKey] = integrity
    await writeJsonFile(opts.filesIndexFile, filesIndex, { indent: undefined })
  }
}
