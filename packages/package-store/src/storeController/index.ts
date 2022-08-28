import {
  PackageFilesIndex,
} from '@pnpm/cafs'
import createCafsStore from '@pnpm/create-cafs-store'
import { Fetchers } from '@pnpm/fetcher-base'
import createPackageRequester from '@pnpm/package-requester'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  ImportIndexedPackage,
  PackageFileInfo,
  StoreController,
} from '@pnpm/store-controller-types'
import loadJsonFile from 'load-json-file'
import writeJsonFile from 'write-json-file'
import prune from './prune'

export default async function (
  resolve: ResolveFunction,
  fetchers: Fetchers,
  initOpts: {
    engineStrict?: boolean
    force?: boolean
    nodeVersion?: string
    importPackage?: ImportIndexedPackage
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
  const cafs = createCafsStore(storeDir, initOpts)
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
    importPackage: cafs.importPackage,
    prune: prune.bind(null, { storeDir, cacheDir: initOpts.cacheDir }),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function upload (builtPkgLocation: string, opts: {filesIndexFile: string, sideEffectsCacheKey: string}) {
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
