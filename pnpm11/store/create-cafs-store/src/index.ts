import { promises as fs } from 'node:fs'
import path from 'node:path'

import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import {
  type CafsLocker,
  createCafs,
} from '@pnpm/store.cafs'
import type { Cafs, FilesMap, PackageFilesResponse } from '@pnpm/store.cafs-types'
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
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const willBeBuilt = !isBuilt && opts.requiresBuild === true
    const pkgImportMethod = selectPackageImportMethod(packageImportMethod, willBeBuilt, opts.filesResponse)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: opts.safeToSkip,
      sourceExists: opts.filesResponse.sourceExists,
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
    const willBeBuilt = !isBuilt && opts.requiresBuild === true
    const pkgImportMethod = selectPackageImportMethod(packageImportMethod, willBeBuilt, opts.filesResponse)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: opts.safeToSkip,
      sourceExists: opts.filesResponse.sourceExists,
    })
    return { importMethod, isBuilt }
  }
}

type ConfiguredImportMethod = 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'

/**
 * A `local-dir` package is a workspace tree a watcher may be reading through
 * an injected copy. Hardlink it when nothing will build it, and copy it when
 * something will, so a copy-on-write clone does not hide later writes.
 */
function selectPackageImportMethod (
  configured: ConfiguredImportMethod | undefined,
  willBeBuilt: boolean,
  filesResponse: { packageImportMethod?: ConfiguredImportMethod, resolvedFrom?: string }
): ConfiguredImportMethod | undefined {
  if (filesResponse.resolvedFrom === 'local-dir') {
    if (willBeBuilt) return 'copy'
    if (configured == null || configured === 'auto' || configured === 'clone' || configured === 'clone-or-copy') {
      return 'hardlink'
    }
  }
  if (willBeBuilt) return configured === 'copy' ? 'copy' : 'clone-or-copy'
  if (configured != null && configured !== 'auto') return configured
  return filesResponse.packageImportMethod ?? configured
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
      await fs.mkdir(baseTempDir, { recursive: true })
      return fs.mkdtemp(path.join(baseTempDir, '_tmp_'))
    },
  }
}
