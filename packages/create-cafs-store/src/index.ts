import { promises as fs } from 'fs'
import path from 'path'
import createCafs, {
  getFilePathByModeInCafs,
} from '@pnpm/cafs'
import { PackageFilesResponse } from '@pnpm/fetcher-base'
import {
  ImportPackageFunction,
  PackageFileInfo,
} from '@pnpm/store-controller-types'
import memoize from 'mem'
import pathTemp from 'path-temp'
import createImportPackage from './createImportPackage'

function createPackageImporter (
  opts: {
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    cafsDir: string
  }
): ImportPackageFunction {
  const cachedImporterCreator = memoize(createImportPackage)
  const packageImportMethod = opts.packageImportMethod
  const gfm = getFlatMap.bind(null, opts.cafsDir)
  return async (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const pkgImportMethod = (opts.requiresBuild && !isBuilt)
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = await impPkg(to, { filesMap, fromStore: opts.filesResponse.fromStore, force: opts.force })
    return { importMethod, isBuilt }
  }
}

function getFlatMap (
  cafsDir: string,
  filesResponse: PackageFilesResponse,
  targetEngine?: string
): { filesMap: Record<string, string>, isBuilt: boolean } {
  if (filesResponse.local) {
    return {
      filesMap: filesResponse.filesIndex,
      isBuilt: false,
    }
  }
  let isBuilt!: boolean
  let filesIndex!: Record<string, PackageFileInfo>
  if (targetEngine && ((filesResponse.sideEffects?.[targetEngine]) != null)) {
    filesIndex = filesResponse.sideEffects?.[targetEngine]
    isBuilt = true
  } else {
    filesIndex = filesResponse.filesIndex
    isBuilt = false
  }
  const filesMap = {}
  for (const [fileName, fileMeta] of Object.entries(filesIndex)) {
    filesMap[fileName] = getFilePathByModeInCafs(cafsDir, fileMeta.integrity, fileMeta.mode)
  }
  return { filesMap, isBuilt }
}

export default function createCafsStore (
  storeDir: string,
  opts?: {
    ignoreFile?: (filename: string) => boolean
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
  }
) {
  const cafsDir = path.join(storeDir, 'files')
  const baseTempDir = path.join(storeDir, 'tmp')
  const importPackage = createPackageImporter({
    packageImportMethod: opts?.packageImportMethod,
    cafsDir,
  })
  return {
    ...createCafs(cafsDir, opts?.ignoreFile),
    importPackage,
    tempDir: async () => {
      const tmpDir = pathTemp(baseTempDir)
      await fs.mkdir(tmpDir, { recursive: true })
      return tmpDir
    },
  }
}
