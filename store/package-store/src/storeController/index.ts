import { createCafsStore, createPackageImporterAsync, type CafsLocker } from '@pnpm/create-cafs-store'
import { type Fetchers } from '@pnpm/fetcher-base'
import { PnpmError } from '@pnpm/error'
import { createPackageRequester } from '@pnpm/package-requester'
import { type ResolveFunction } from '@pnpm/resolver-base'
import {
  type ImportIndexedPackageAsync,
  type StoreController,
} from '@pnpm/store-controller-types'
import { addFilesFromDir, workerPool as pool } from '@pnpm/worker'
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

  function logIfServer<T> (log: T) {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    if ((globalThis as any).isRunningInPnpmServer) {
      console.log(log)
    }
  }

  return {
    close: async () => {}, // eslint-disable-line:no-empty
    fetchPackage: packageRequester.fetchPackageToStore,
    getFilesIndexFilePath: packageRequester.getFilesIndexFilePath,
    importPackage: initOpts.importPackage
      ? createPackageImporterAsync({ importIndexedPackage: initOpts.importPackage, cafsDir: cafs.cafsDir })
      : async (targetDir, opts) => {
        const beforeCheckingOutWorker = performance.now()
        logIfServer('Checking out worker for "link"')
        logIfServer({
          max: pool.maxWorkers,
          active: pool.getActiveCount(),
          idle: pool.getIdleCount(),
          live: pool.getLiveCount(),
        })
        const localWorker = await pool.checkoutWorkerAsync(true)
        logIfServer(`Finished checking out worker for "link": ${performance.now() - beforeCheckingOutWorker}`)
        return new Promise<{ isBuilt: boolean, importMethod: string | undefined }>((resolve, reject) => {
          localWorker.once('message', ({ status, error, value }: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
            logIfServer('Worker responded with message for link')
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
    await addFilesFromDir({
      cafsDir: cafs.cafsDir,
      dir: builtPkgLocation,
      sideEffectsCacheKey: opts.sideEffectsCacheKey,
      filesIndexFile: opts.filesIndexFile,
    })
  }
}
