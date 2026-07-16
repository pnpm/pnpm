import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

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
    const needsPrivateCopy = makeWritable && await hasStoreHardlinksAsync(to, filesMap)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force || needsPrivateCopy,
      keepModulesDir: Boolean(opts.keepModulesDir),
      safeToSkip: needsPrivateCopy ? false : opts.safeToSkip,
    })
    if (makeWritable) await makePackageWritableAsync(to, filesMap)
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
    const needsPrivateCopy = makeWritable && hasStoreHardlinks(to, filesMap)
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
  const directories = new Set([dir])
  const files: string[] = []
  for (const filename of filesMap.keys()) {
    const candidates = packageFileCandidates(dir, filename)
    const filePath = candidates.find(fs.existsSync) ?? candidates.at(-1)!
    files.push(filePath)
    addParentDirectories(directories, dir, filePath)
  }
  for (const directory of Array.from(directories).sort((a, b) => a.length - b.length)) {
    makePathWritable(directory)
  }
  for (const file of files) makePathWritable(file)
}

function hasStoreHardlinks (dir: string, filesMap: FilesMap): boolean {
  if (!fs.existsSync(path.join(dir, 'package.json'))) return false
  for (const [filename, storePath] of filesMap) {
    const targetPath = packageFileCandidates(dir, filename).find(fs.existsSync)
    if (targetPath == null) return true
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

async function makePackageWritableAsync (dir: string, filesMap: FilesMap): Promise<void> {
  const directories = new Set([dir])
  const files = await mapInBatches(Array.from(filesMap.keys()), async (filename) => {
    const candidates = packageFileCandidates(dir, filename)
    const filePath = await findExistingPath(candidates) ?? candidates.at(-1)!
    addParentDirectories(directories, dir, filePath)
    return filePath
  })
  await mapInBatches(Array.from(directories).sort((a, b) => a.length - b.length), makePathWritableAsync)
  await mapInBatches(files, makePathWritableAsync)
}

async function hasStoreHardlinksAsync (dir: string, filesMap: FilesMap): Promise<boolean> {
  if (await findExistingPath([path.join(dir, 'package.json')]) == null) return false
  const entries = Array.from(filesMap)
  for (let index = 0; index < entries.length; index += FS_OPERATION_CONCURRENCY) {
    const results = await Promise.all(entries.slice(index, index + FS_OPERATION_CONCURRENCY).map(async ([filename, storePath]) => { // eslint-disable-line no-await-in-loop
      const candidates = packageFileCandidates(dir, filename)
      try {
        const targetPath = await findExistingPath(candidates)
        if (targetPath == null) return true
        const [targetStat, storeStat] = await Promise.all([
          fs.promises.stat(targetPath),
          fs.promises.stat(storePath),
        ])
        return targetStat.dev === storeStat.dev && targetStat.ino === storeStat.ino
      } catch {
        return true
      }
    }))
    if (results.some(Boolean)) return true
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

async function findExistingPath (candidates: string[]): Promise<string | undefined> {
  for (const candidate of candidates) {
    try {
      // eslint-disable-next-line no-await-in-loop
      await fs.promises.lstat(candidate)
      return candidate
    } catch (err: unknown) {
      if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
    }
  }
  return undefined
}

const FS_OPERATION_CONCURRENCY = 64

async function mapInBatches<T, R> (
  items: T[],
  operation: (item: T) => Promise<R>
): Promise<R[]> {
  const results: R[] = []
  for (let index = 0; index < items.length; index += FS_OPERATION_CONCURRENCY) {
    results.push(...await Promise.all(items.slice(index, index + FS_OPERATION_CONCURRENCY).map(operation))) // eslint-disable-line no-await-in-loop
  }
  return results
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

async function makePathWritableAsync (filePath: string): Promise<void> {
  if (process.platform === 'win32') {
    const mode = (await fs.promises.lstat(filePath)).mode
    if ((mode & 0o200) === 0) await fs.promises.chmod(filePath, mode | 0o200)
    return
  }
  let handle: fs.promises.FileHandle | undefined
  try {
    handle = await fs.promises.open(filePath, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW)
    const mode = (await handle.stat()).mode
    if ((mode & 0o200) === 0) await handle.chmod(mode | 0o200)
  } finally {
    await handle?.close()
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
