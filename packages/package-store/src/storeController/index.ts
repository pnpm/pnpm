import {
  getFilePathByModeInCafs as _getFilePathByModeInCafs,
  PackageFileInfo,
  PackageFilesIndex,
} from '@pnpm/cafs'
import { FetchFunction } from '@pnpm/fetcher-base'
import createPackageRequester, { getCacheByEngine } from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  ImportPackageFunction,
  StoreController,
} from '@pnpm/store-controller-types'
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import pFilter = require('p-filter')
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import writeJsonFile = require('write-json-file')
import createImportPackage from './createImportPackage'
import prune from './prune'

export default async function (
  resolve: ResolveFunction,
  fetchers: {[type: string]: FetchFunction},
  initOpts: {
    ignoreFile?: (filename: string) => boolean,
    storeDir: string,
    networkConcurrency?: number,
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone',
    verifyStoreIntegrity: boolean,
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
    await impPkg(to, { filesMap, fromStore: opts.filesResponse.fromStore, force: opts.force })
    return { isBuilt }
  }

  return {
    close: async () => {}, // tslint:disable-line:no-empty
    fetchPackage: packageRequester.fetchPackageToStore,
    getPackageLocation,
    importPackage,
    prune: prune.bind(null, storeDir),
    requestPackage: packageRequester.requestPackage,
    upload,
  }

  async function getPackageLocation (
    packageId: string,
    packageName: string,
    opts: {
      lockfileDir: string,
      targetEngine?: string,
    }
  ) {
    if (opts.targetEngine) {
      const sideEffectsCacheLocation = (await getCacheByEngine(initOpts.storeDir, packageId))[opts.targetEngine]
      if (sideEffectsCacheLocation) {
        return {
          dir: sideEffectsCacheLocation,
          isBuilt: true,
        }
      }
    }

    return {
      dir: path.join(initOpts.storeDir, pkgIdToFilename(packageId, opts.lockfileDir), 'node_modules', packageName),
      isBuilt: false,
    }
  }

  async function upload (builtPkgLocation: string, opts: {filesIndexFile: string, engine: string}) {
    const sideEffectsIndex = await packageRequester.cafs.addFilesFromDir(builtPkgLocation)
    // TODO: move this to a function
    // This is duplicated in @pnpm/package-requester
    const integrity = {}
    await Promise.all(
      Object.keys(sideEffectsIndex)
        .map(async (filename) => {
          const fileIntegrity = await sideEffectsIndex[filename].generatingIntegrity
          integrity[filename] = {
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
