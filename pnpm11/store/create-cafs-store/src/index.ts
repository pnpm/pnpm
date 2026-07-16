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
    const needsPrivateCopy = makeWritable && packageNeedsPrivateCopy(to, filesMap)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force || needsPrivateCopy,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: needsPrivateCopy ? false : opts.safeToSkip,
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
    const needsPrivateCopy = makeWritable && packageNeedsPrivateCopy(to, filesMap)
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force || needsPrivateCopy,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: needsPrivateCopy ? false : opts.safeToSkip,
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
  const packageDir = path.resolve(dir)
  const directories = new Set([packageDir])
  const files: string[] = []
  for (const filename of filesMap.keys()) {
    const candidates = packageFileCandidates(packageDir, filename)
    const filePath = candidates.find(fs.existsSync) ?? candidates.at(-1)!
    files.push(filePath)
    addParentDirectories(directories, packageDir, filePath)
  }
  for (const directory of Array.from(directories).sort((a, b) => a.length - b.length)) {
    makePathWritable(directory)
  }
  for (const file of files) makePathWritable(file)
}

function packageNeedsPrivateCopy (dir: string, filesMap: FilesMap): boolean {
  if (!fs.existsSync(dir)) return false
  try {
    if (!fs.lstatSync(dir).isDirectory()) return true
  } catch {
    return true
  }
  for (const [filename, storePath] of filesMap) {
    const targetPath = packageFileCandidates(dir, filename).find(fs.existsSync)
    if (targetPath == null) return true
    try {
      const targetStat = fs.lstatSync(targetPath, { bigint: true })
      if (!targetStat.isFile()) return true
      const storeStat = fs.statSync(storePath, { bigint: true })
      if (targetStat.ino === 0n || storeStat.ino === 0n) return true
      if (targetStat.dev === storeStat.dev && targetStat.ino === storeStat.ino) return true
    } catch {
      return true
    }
  }
  return false
}

function packageFileCandidates (dir: string, filename: string): string[] {
  const originalPath = path.join(dir, filename)
  const sanitizedPath = path.join(dir, sanitizeFilenamePath(filename))
  const candidates = originalPath === sanitizedPath ? [originalPath] : [originalPath, sanitizedPath]
  for (const candidate of candidates) assertPathInsideProjection(dir, candidate)
  return candidates
}

function assertPathInsideProjection (dir: string, filePath: string): void {
  const relativePath = path.relative(dir, filePath)
  if (relativePath === '' || relativePath === '..' || relativePath.startsWith(`..${path.sep}`) || path.isAbsolute(relativePath)) {
    throw new Error(`Cannot make package file outside the projection writable: ${filePath}`)
  }
}

function addParentDirectories (directories: Set<string>, dir: string, filePath: string): void {
  let parent = path.dirname(filePath)
  while (parent !== dir) {
    directories.add(parent)
    parent = path.dirname(parent)
  }
}

function makePathWritable (filePath: string): void {
  const pathStat = fs.lstatSync(filePath, { bigint: true })
  if (pathStat.isSymbolicLink()) {
    throw new Error(`Cannot make symlinked package file writable: ${filePath}`)
  }
  const openFlags = process.platform === 'win32'
    ? fs.constants.O_RDONLY
    : fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW
  const fd = fs.openSync(filePath, openFlags)
  try {
    const openedStat = fs.fstatSync(fd, { bigint: true })
    if (process.platform === 'win32' && (
      pathStat.ino === 0n || openedStat.ino === 0n ||
      pathStat.dev !== openedStat.dev || pathStat.ino !== openedStat.ino
    )) {
      throw new Error(`Package file changed while making it writable: ${filePath}`)
    }
    const mode = Number(openedStat.mode)
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
