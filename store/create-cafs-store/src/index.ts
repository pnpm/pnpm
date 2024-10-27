import { promises as fs } from 'fs'
import path from 'path'
import {
  type CafsLocker,
  createCafs,
  getFilePathByModeInCafs,
} from '@pnpm/store.cafs'
import { type Cafs, type PackageFilesResponse, type PackageFiles, type SideEffectsDiff } from '@pnpm/cafs-types'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import {
  type ImportIndexedPackage,
  type ImportIndexedPackageAsync,
  type ImportPackageFunction,
  type ImportPackageFunctionAsync,
} from '@pnpm/store-controller-types'
import memoize from 'mem'
import pathTemp from 'path-temp'
import mapValues from 'ramda/src/map'

export { type CafsLocker }

export function createPackageImporterAsync (
  opts: {
    importIndexedPackage?: ImportIndexedPackageAsync
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    storeDir: string
  }
): ImportPackageFunctionAsync {
  const cachedImporterCreator = opts.importIndexedPackage
    ? () => opts.importIndexedPackage!
    : memoize(createIndexedPkgImporter)
  const packageImportMethod = opts.packageImportMethod
  const gfm = getFlatMap.bind(null, opts.storeDir)
  return async (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const willBeBuilt = !isBuilt && opts.requiresBuild
    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })
    return { importMethod, isBuilt }
  }
}

function createPackageImporter (
  opts: {
    importIndexedPackage?: ImportIndexedPackage
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    storeDir: string
  }
): ImportPackageFunction {
  const cachedImporterCreator = opts.importIndexedPackage
    ? () => opts.importIndexedPackage!
    : memoize(createIndexedPkgImporter)
  const packageImportMethod = opts.packageImportMethod
  const gfm = getFlatMap.bind(null, opts.storeDir)
  return (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const willBeBuilt = !isBuilt && opts.requiresBuild
    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })
    return { importMethod, isBuilt }
  }
}

function getFlatMap (
  storeDir: string,
  filesResponse: PackageFilesResponse,
  targetEngine?: string
): { filesMap: Record<string, string>, isBuilt: boolean } {
  let isBuilt!: boolean
  let filesIndex!: PackageFiles
  if (targetEngine && ((filesResponse.sideEffects?.[targetEngine]) != null)) {
    filesIndex = applySideEffectsDiff(filesResponse.filesIndex as PackageFiles, filesResponse.sideEffects?.[targetEngine])
    isBuilt = true
  } else if (!filesResponse.unprocessed) {
    return {
      filesMap: filesResponse.filesIndex,
      isBuilt: false,
    }
  } else {
    filesIndex = filesResponse.filesIndex
    isBuilt = false
  }
  const filesMap = mapValues(({ integrity, mode }) => getFilePathByModeInCafs(storeDir, integrity, mode), filesIndex)
  return { filesMap, isBuilt }
}

function applySideEffectsDiff (baseFiles: PackageFiles, { added, deleted }: SideEffectsDiff): PackageFiles {
  const filesWithSideEffects: PackageFiles = { ...added }
  for (const fileName in baseFiles) {
    if (!deleted?.includes(fileName) && !filesWithSideEffects[fileName]) {
      filesWithSideEffects[fileName] = baseFiles[fileName]
    }
  }
  return filesWithSideEffects
}

export function createCafsStore (
  storeDir: string,
  opts?: {
    ignoreFile?: (filename: string) => boolean
    importPackage?: ImportIndexedPackage
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    cafsLocker?: CafsLocker
  }
): Cafs {
  const baseTempDir = path.join(storeDir, 'tmp')
  const importPackage = createPackageImporter({
    importIndexedPackage: opts?.importPackage,
    packageImportMethod: opts?.packageImportMethod,
    storeDir,
  })
  return {
    ...createCafs(storeDir, opts),
    storeDir,
    importPackage,
    tempDir: async () => {
      const tmpDir = pathTemp(baseTempDir)
      await fs.mkdir(tmpDir, { recursive: true })
      return tmpDir
    },
  }
}
