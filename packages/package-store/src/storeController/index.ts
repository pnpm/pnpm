import {
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  PackageFileInfo,
  PackageFilesIndex,
} from '@pnpm/cafs'
import { FetchFunction } from '@pnpm/fetcher-base'
import createPackageRequester from '@pnpm/package-requester'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  ImportPackageFunction,
  StoreController,
} from '@pnpm/store-controller-types'
import createImportPackage from './createImportPackage'
import prune from './prune'
import path = require('path')
import loadJsonFile = require('load-json-file')
import writeJsonFile = require('write-json-file')

export default async function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  initOpts: {
    ignoreFile?: (filename: string) => boolean
    storeDir: string
    networkConcurrency?: number
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
    verifyStoreIntegrity: boolean
  }
): Promise<StoreController> {
  const storeDir = initOpts.storeDir
  const packageRequester = createPackageRequester(resolve, fetchers, {
    ignoreFile: initOpts.ignoreFile,
    networkConcurrency: initOpts.networkConcurrency,
    storeDir: initOpts.storeDir,
    verifyStoreIntegrity: initOpts.verifyStoreIntegrity,
  })

  const impPkg = createImportPackage(initOpts.packageImportMethod)
  const cafsDir = path.join(storeDir, 'files')
  const getFilePathByModeInCafs = _getFilePathByModeInCafs.bind(null, cafsDir)
  const importPackage: ImportPackageFunction = async (to, opts) => {
    const filesMap = {} as Record<string, string>
    let isBuilt!: boolean
    let filesIndex!: Record<string, PackageFileInfo>
    if (opts.targetEngine && opts.filesResponse.sideEffects?.[opts.targetEngine]) {
      filesIndex = opts.filesResponse.sideEffects?.[opts.targetEngine]
      isBuilt = true
    } else {
      filesIndex = opts.filesResponse.filesIndex
      isBuilt = false
    }
    for (const [fileName, fileMeta] of Object.entries(filesIndex)) {
      filesMap[fileName] = getFilePathByModeInCafs(fileMeta.integrity, fileMeta.mode)
    }
    const importMethod = await impPkg(to, { filesMap, fromStore: opts.filesResponse.fromStore, force: opts.force })
    return { importMethod, isBuilt }
  }

  return {
    close: async () => {}, // eslint-disable-line:no-empty
    fetchPackage: packageRequester.fetchPackageToStore,
    importPackage,
    prune: prune.bind(null, storeDir),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function upload (builtPkgLocation: string, opts: {filesIndexFile: string, engine: string}) {
    const sideEffectsIndex = await packageRequester.cafs.addFilesFromDir(builtPkgLocation)
    // TODO: move this to a function
    // This is duplicated in @pnpm/package-requester
    const integrity: Record<string, PackageFileInfo> = {}
    await Promise.all(
      Object.keys(sideEffectsIndex)
        .map(async (filename) => {
          const {
            checkedAt,
            integrity: fileIntegrity,
          } = await sideEffectsIndex[filename].writeResult
          integrity[filename] = {
            checkedAt,
            integrity: fileIntegrity.toString(), // TODO: use the raw Integrity object
            mode: sideEffectsIndex[filename].mode,
            size: sideEffectsIndex[filename].size,
          }
        })
    )
    let filesIndex!: PackageFilesIndex
    try {
      filesIndex = await loadJsonFile<PackageFilesIndex>(opts.filesIndexFile)
    } catch (err) {
      filesIndex = { files: integrity }
    }
    filesIndex.sideEffects = filesIndex.sideEffects ?? {}
    filesIndex.sideEffects[opts.engine] = integrity
    await writeJsonFile(opts.filesIndexFile, filesIndex, { indent: undefined })
  }
}
