import fs from 'node:fs'
import path from 'node:path'

import { createIndexedPkgImporter, sanitizeFilenamePath } from '@pnpm/fs.indexed-pkg-importer'
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
  const usesCustomImporter = opts.importIndexedPackage != null
  const gfm = getFlatMap.bind(null, opts.storeDir)
  return async (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const mayBeMutated = opts.requiresBuild === true
    const makeWritable = mayBeMutated && !usesCustomImporter
    const pkgImportMethod = mayBeMutated
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const safeToSkip = makeWritable && hasStoreHardlinks(to, filesMap)
      ? false
      : opts.safeToSkip
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force || makeWritable,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip,
    })
    if (makeWritable) makePackageWritable(to, filesMap)
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
  const usesCustomImporter = opts.importIndexedPackage != null
  const gfm = getFlatMap.bind(null, opts.storeDir)
  return (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const mayBeMutated = opts.requiresBuild === true
    const makeWritable = mayBeMutated && !usesCustomImporter
    const pkgImportMethod = mayBeMutated
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const safeToSkip = makeWritable && hasStoreHardlinks(to, filesMap)
      ? false
      : opts.safeToSkip
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force || makeWritable,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip,
    })
    if (makeWritable) makePackageWritable(to, filesMap)
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

function makePackageWritable (dir: string, filesMap: FilesMap): void {
  const directories = new Set([dir])
  const files: string[] = []
  for (const filename of filesMap.keys()) {
    const originalPath = path.join(dir, filename)
    const sanitizedPath = path.join(dir, sanitizeFilenamePath(filename))
    const filePath = fs.existsSync(originalPath) ? originalPath : sanitizedPath
    const relativePath = path.relative(dir, filePath)
    if (relativePath === '..' || relativePath.startsWith(`..${path.sep}`) || path.isAbsolute(relativePath)) {
      throw new Error(`Cannot make package file outside the projection writable: ${filePath}`)
    }
    files.push(filePath)
    let parent = path.dirname(filePath)
    while (parent !== dir) {
      directories.add(parent)
      parent = path.dirname(parent)
    }
  }
  for (const directory of Array.from(directories).sort((a, b) => a.length - b.length)) {
    makePathWritable(directory)
  }
  for (const file of files) makePathWritable(file)
}

function hasStoreHardlinks (dir: string, filesMap: FilesMap): boolean {
  for (const [filename, storePath] of filesMap) {
    const targetPath = path.join(dir, filename)
    if (!fs.existsSync(targetPath)) return false
    try {
      const targetStat = fs.statSync(targetPath)
      const storeStat = fs.statSync(storePath)
      if (targetStat.dev === storeStat.dev && targetStat.ino === storeStat.ino) return true
    } catch {
      return true
    }
  }
  return false
}

function makePathWritable (filePath: string): void {
  if (process.platform === 'win32') {
    const mode = fs.lstatSync(filePath).mode
    if ((mode & 0o200) === 0) fs.chmodSync(filePath, mode | 0o200)
    return
  }
  const fd = fs.openSync(filePath, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW)
  try {
    const mode = fs.fstatSync(fd).mode
    if ((mode & 0o200) === 0) fs.fchmodSync(fd, mode | 0o200)
  } finally {
    fs.closeSync(fd)
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
      await fs.promises.mkdir(baseTempDir, { recursive: true })
      return fs.promises.mkdtemp(path.join(baseTempDir, '_tmp_'))
    },
  }
}
