import { getFilePathByModeInCafs as _getFilePathByModeInCafs } from '@pnpm/cafs'
import { FetchFunction } from '@pnpm/fetcher-base'
import createPackageRequester, { getCacheByEngine } from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ResolveFunction } from '@pnpm/resolver-base'
import {
  ImportPackageFunction,
  StoreController,
} from '@pnpm/store-controller-types'
import rimraf = require('@zkochan/rimraf')
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
  const importPackage: ImportPackageFunction = (to, opts) => {
    const filesMap = {} as Record<string, string>
    for (const [fileName, fileMeta] of Object.entries(opts.filesResponse.filesIndex)) {
      filesMap[fileName] = getFilePathByModeInCafs(fileMeta.integrity, fileMeta.mode)
    }
    return impPkg(to, { filesMap, fromStore: opts.filesResponse.fromStore, force: opts.force })
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

  async function upload (builtPkgLocation: string, opts: {packageId: string, engine: string}) {
    const filesIndex = await packageRequester.cafs.addFilesFromDir(builtPkgLocation)
    // TODO: move this to a function
    // This is duplicated in @pnpm/package-requester
    const integrity = {}
    await Promise.all(
      Object.keys(filesIndex)
        .map(async (filename) => {
          const fileIntegrity = await filesIndex[filename].generatingIntegrity
          integrity[filename] = {
            integrity: fileIntegrity.toString(), // TODO: use the raw Integrity object
            mode: filesIndex[filename].mode,
            size: filesIndex[filename].size,
          }
        })
    )
    const cachePath = path.join(storeDir, opts.packageId, 'side_effects', opts.engine)
    await writeJsonFile(path.join(cachePath, 'integrity.json'), integrity, { indent: undefined })
  }
}
