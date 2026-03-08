import { promises as fs } from 'fs'
import path from 'path'
import {
  type CafsLocker,
  createCafs,
} from '@pnpm/store.cafs'
import type { Cafs, PackageFilesResponse, FilesMap } from '@pnpm/cafs-types'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import type {
  ImportIndexedPackage,
  ImportIndexedPackageAsync,
  ImportPackageFunction,
  ImportPackageFunctionAsync,
} from '@pnpm/store-controller-types'
import memoize from 'memoize'
import pathTemp from 'path-temp'

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
): { filesMap: FilesMap, isBuilt: boolean } {
  if (targetEngine && filesResponse.sideEffectsMaps?.has(targetEngine)) {
    const sideEffectMap = filesResponse.sideEffectsMaps.get(targetEngine)!
    const filesMap = applySideEffectsDiffWithMaps(filesResponse.filesMap, sideEffectMap)
    return {
      filesMap,
      isBuilt: true,
    }
  }
  return {
    filesMap: filesResponse.filesMap,
    isBuilt: false,
  }
}

// Apply side effects when we already have file location maps (fast path)
function applySideEffectsDiffWithMaps (
  baseFiles: FilesMap,
  { added, deleted }: { added?: FilesMap, deleted?: string[] }
): FilesMap {
  const filesWithSideEffects = new Map<string, string>()
  // Add side effect files (already have file paths)
  if (added) {
    for (const [name, filePath] of added.entries()) {
      filesWithSideEffects.set(name, filePath)
    }
  }
  // Add base files that weren't deleted
  for (const [fileName, filePath] of baseFiles) {
    if (!deleted?.includes(fileName) && !filesWithSideEffects.has(fileName)) {
      filesWithSideEffects.set(fileName, filePath)
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
