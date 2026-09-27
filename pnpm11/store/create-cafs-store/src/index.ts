import { promises as fs } from 'node:fs'
import path from 'node:path'

import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import {
  type CafsLocker,
  createCafs,
} from '@pnpm/store.cafs'
import type { Cafs, FilesMap, PackageFilesResponse, SideEffectsFilesMap } from '@pnpm/store.cafs-types'
import type {
  ImportIndexedPackage,
  ImportIndexedPackageAsync,
  ImportPackageFunction,
  ImportPackageFunctionAsync,
} from '@pnpm/store.controller-types'
import memoize from 'memoize'

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
    const { filesMap, isBuilt, symlinks } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const pkgImportMethod = needsPrivateFiles(opts.filesResponse, !isBuilt && opts.requiresBuild === true)
      ? (packageImportMethod === 'copy' ? 'copy' : 'clone-or-copy')
      : (packageImportMethod && packageImportMethod !== 'auto'
        ? packageImportMethod
        : (opts.filesResponse.packageImportMethod ?? packageImportMethod))
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: opts.safeToSkip,
      sourceExists: opts.filesResponse.sourceExists,
      symlinks,
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
    const { filesMap, isBuilt, symlinks } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const pkgImportMethod = needsPrivateFiles(opts.filesResponse, !isBuilt && opts.requiresBuild === true)
      ? (packageImportMethod === 'copy' ? 'copy' : 'clone-or-copy')
      : (packageImportMethod && packageImportMethod !== 'auto'
        ? packageImportMethod
        : (opts.filesResponse.packageImportMethod ?? packageImportMethod))
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: opts.safeToSkip,
      sourceExists: opts.filesResponse.sourceExists,
      symlinks,
    })
    return { importMethod, isBuilt }
  }
}

// A package that a build will still write to must not share inodes with its
// source. Neither may a local directory whose fetcher asked for private copies
// (`pnpm deploy`), even when a global `packageImportMethod` asks for hard links.
function needsPrivateFiles (filesResponse: PackageFilesResponse, willBeBuilt: boolean): boolean {
  return willBeBuilt ||
    (filesResponse.resolvedFrom === 'local-dir' && filesResponse.packageImportMethod === 'clone-or-copy')
}

function getFlatMap (
  storeDir: string,
  filesResponse: PackageFilesResponse,
  targetEngine?: string
): { filesMap: FilesMap, isBuilt: boolean, symlinks?: Map<string, string> } {
  if (targetEngine && filesResponse.sideEffectsMaps?.has(targetEngine)) {
    const sideEffectMap = filesResponse.sideEffectsMaps.get(targetEngine)!
    const filesMap = applySideEffectsDiffWithMaps(filesResponse.filesMap, sideEffectMap)
    return {
      filesMap,
      isBuilt: true,
      symlinks: sideEffectMap.symlinks,
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
  { added, deleted, symlinks }: SideEffectsFilesMap
): FilesMap {
  const filesWithSideEffects = new Map<string, string>()
  // Add side effect files (already have file paths)
  if (added) {
    for (const [name, filePath] of added.entries()) {
      filesWithSideEffects.set(name, filePath)
    }
  }
  // Add base files that weren't deleted or replaced by a symlink
  for (const [fileName, filePath] of baseFiles) {
    if (!deleted?.includes(fileName) && !filesWithSideEffects.has(fileName) && !symlinks?.has(fileName)) {
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
      await fs.mkdir(baseTempDir, { recursive: true })
      return fs.mkdtemp(path.join(baseTempDir, '_tmp_'))
    },
  }
}
