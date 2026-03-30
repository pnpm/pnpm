import assert from 'node:assert'
import { constants, existsSync, type Stats } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { packageImportMethodLogger } from '@pnpm/core-loggers'
import fs from '@pnpm/fs.graceful-fs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import type { FilesMap, ImportIndexedPackage, ImportOptions } from '@pnpm/store.controller-types'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'

import { type Importer, type ImportFile, importIndexedDir } from './importIndexedDir.js'

export { type FilesMap, type ImportIndexedPackage, type ImportOptions }

export type PackageImportMethod = 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'

export function createIndexedPkgImporter (packageImportMethod?: PackageImportMethod): ImportIndexedPackage {
  const importPackage = createImportPackage(packageImportMethod)
  return importPackage
}

function createImportPackage (packageImportMethod?: PackageImportMethod): ImportIndexedPackage {
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod ?? 'auto') {
    case 'clone':
      packageImportMethodLogger.debug({ method: 'clone' })
      return createClonePkg()
    case 'hardlink':
      packageImportMethodLogger.debug({ method: 'hardlink' })
      return hardlinkPkg.bind(null, linkOrCopy)
    case 'auto': {
      return createAutoImporter()
    }
    case 'clone-or-copy':
      return createCloneOrCopyImporter()
    case 'copy':
      packageImportMethodLogger.debug({ method: 'copy' })
      return copyPkg
    default:
      throw new Error(`Unknown package import method ${packageImportMethod as string}`)
  }
}

function createAutoImporter (): ImportIndexedPackage {
  let auto = initialAuto

  return (to, opts) => auto(to, opts)

  function initialAuto (
    to: string,
    opts: ImportOptions
  ): string | undefined {
    // Although reflinks are supported on Windows Dev Drives,
    // they are 10x slower than hard links.
    // Hence, we prefer reflinks by default only on Linux and macOS.
    if (process.platform !== 'win32') {
      try {
        // Probe with the raw clone function (no ENOTSUP fallback).
        // On filesystems that don't support reflinks (e.g. ext4), this
        // throws and we fall through to hardlinks — which is much faster
        // than copying.  If the probe succeeds, we switch to the full
        // clone importer (with ENOTSUP fallback for transient failures
        // during heavy parallel I/O) for all subsequent packages.
        const clone = createCloneFunction()
        if (!clonePkgRaw(clone, to, opts)) return undefined
        packageImportMethodLogger.debug({ method: 'clone' })
        auto = createClonePkg()
        return 'clone'
      } catch {
        // ignore
      }
    }
    try {
      if (!hardlinkPkg(fs.linkSync, to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return 'hardlink'
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      if (err.message.startsWith('EXDEV: cross-device link not permitted')) {
        globalWarn(err.message)
        globalInfo('Falling back to copying packages from store')
        packageImportMethodLogger.debug({ method: 'copy' })
        auto = copyPkg
        return auto(to, opts)
      }
      // We still choose hard linking that will fall back to copying in edge cases.
      packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return auto(to, opts)
    }
  }
}

function createCloneOrCopyImporter (): ImportIndexedPackage {
  let auto = initialAuto

  return (to, opts) => auto(to, opts)

  function initialAuto (
    to: string,
    opts: ImportOptions
  ): string | undefined {
    try {
      const clone = createCloneFunction()
      if (!clonePkgRaw(clone, to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'clone' })
      auto = createClonePkg()
      return 'clone'
    } catch {
      // ignore
    }
    packageImportMethodLogger.debug({ method: 'copy' })
    auto = copyPkg
    return auto(to, opts)
  }
}

type CloneFunction = (src: string, dest: string) => void

/**
 * Import a single package using a raw clone function (no ENOTSUP fallback).
 * Used by auto-mode to probe whether the filesystem supports cloning.
 * If cloning isn't supported, the error propagates so the caller can fall
 * through to a faster method (e.g. hardlinks).
 */
function clonePkgRaw (
  clone: CloneFunction,
  to: string,
  opts: ImportOptions
): 'clone' | undefined {
  if (opts.resolvedFrom !== 'store' || opts.force || !pkgExistsAtTargetDir(to, opts.filesMap)) {
    importIndexedDir({ importFile: clone, importFileAtomic: clone }, to, opts.filesMap, opts)
    return 'clone'
  }
  return undefined
}

/**
 * Creates a clone-based package importer.  Reflinks are atomic, so clone can
 * serve as both importFile and importFileAtomic.  However, on Linux
 * copy_file_range can transiently fail with ENOTSUP under heavy parallel I/O,
 * so we fall back to copy on ENOTSUP.  Regular files use a simple copy;
 * package.json (the completion marker) uses a temp+rename fallback to stay
 * atomic.
 */
function createClonePkg (): ImportIndexedPackage {
  const clone = createCloneFunction()
  const withFallback = (fallback: CloneFunction): ImportFile => (src, dest) => {
    try {
      clone(src, dest)
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOTSUP') {
        fallback(src, dest)
        return
      }
      throw err
    }
  }
  const importer: Importer = {
    importFile: withFallback(resilientCopyFileSync),
    importFileAtomic: withFallback(atomicCopyFileSync),
  }
  return (to: string, opts: ImportOptions) => {
    if (opts.resolvedFrom !== 'store' || opts.force || !pkgExistsAtTargetDir(to, opts.filesMap)) {
      importIndexedDir(importer, to, opts.filesMap, opts)
      return 'clone'
    }
    return undefined
  }
}

function pkgExistsAtTargetDir (targetDir: string, filesMap: FilesMap): boolean {
  return existsSync(path.join(targetDir, pickFileFromFilesMap(filesMap)))
}

function pickFileFromFilesMap (filesMap: FilesMap): string {
  // New packages always have a package.json (the worker synthesizes one if
  // the tarball/directory lacks it).  The fallback handles old store entries
  // that were indexed before the synthetic package.json was introduced.
  if (filesMap.has('package.json')) {
    return 'package.json'
  }
  if (filesMap.size === 0) {
    throw new Error('pickFileFromFilesMap cannot pick a file from an empty FilesMap')
  }
  return filesMap.keys().next().value!
}

function createCloneFunction (): CloneFunction {
  // Node.js currently does not natively support reflinks on Windows and macOS.
  // Hence, we use a third party solution.
  if (process.platform === 'darwin' || process.platform === 'win32') {
    // eslint-disable-next-line
    const { reflinkFileSync } = require('@reflink/reflink') as typeof import('@reflink/reflink')
    return (fr, to) => {
      try {
        reflinkFileSync(fr, to)
      } catch (err: unknown) {
        // If the file already exists, then we just proceed.
        // This will probably only happen if the package's index file contains the same file twice.
        // For instance: { "index.js": "hash", "./index.js": "hash" }
        if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'EEXIST') throw err
      }
    }
  }
  return (src: string, dest: string) => {
    try {
      fs.copyFileSync(src, dest, constants.COPYFILE_FICLONE_FORCE)
    } catch (err: unknown) {
      if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
    }
  }
}

function hardlinkPkg (
  importFile: ImportFile,
  to: string,
  opts: ImportOptions
): 'hardlink' | undefined {
  if (opts.force || shouldRelinkPkg(to, opts)) {
    importIndexedDir({ importFile, importFileAtomic: importFile }, to, opts.filesMap, opts)
    return 'hardlink'
  }
  return undefined
}

function shouldRelinkPkg (
  to: string,
  opts: ImportOptions
): boolean {
  if (opts.disableRelinkLocalDirDeps && opts.resolvedFrom === 'local-dir') {
    try {
      const files = fs.readdirSync(to)
      return files.length === 0 || files.length === 1 && files[0] === 'node_modules'
    } catch {
      return true
    }
  }
  return opts.resolvedFrom !== 'store' || !pkgLinkedToStore(opts.filesMap, to)
}

function linkOrCopy (existingPath: string, newPath: string): void {
  try {
    fs.linkSync(existingPath, newPath)
  } catch (err: unknown) {
    // If a hard link to the same file already exists
    // then trying to copy it will make an empty file from it.
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') return
    // In some VERY rare cases (1 in a thousand), hard-link creation fails on Windows.
    // In that case, we just fall back to copying.
    // This issue is reproducible with "pnpm add @material-ui/icons@4.9.1"
    resilientCopyFileSync(existingPath, newPath)
  }
}

// On Linux CI, the kernel's copy_file_range/sendfile can transiently fail
// with ENOTSUP under heavy parallel I/O on the same store files.
// Fall back to manual read+write which uses plain read/write syscalls.
function resilientCopyFileSync (src: string, dest: string): void {
  try {
    fs.copyFileSync(src, dest)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOTSUP') {
      const srcMode = fs.statSync(src).mode
      fs.writeFileSync(dest, fs.readFileSync(src), { mode: srcMode })
    } else {
      throw err
    }
  }
}

function pkgLinkedToStore (filesMap: FilesMap, linkedPkgDir: string): boolean {
  const filename = pickFileFromFilesMap(filesMap)
  const linkedFile = path.join(linkedPkgDir, filename)
  let stats0!: Stats
  try {
    stats0 = fs.statSync(linkedFile)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
  }
  const stats1 = fs.statSync(filesMap.get(filename)!)
  if (stats0.ino === stats1.ino) return true
  globalInfo(`Relinking ${linkedPkgDir} from the store`)
  return false
}

export function copyPkg (
  to: string,
  opts: ImportOptions
): 'copy' | undefined {
  if (opts.resolvedFrom !== 'store' || opts.force || !pkgExistsAtTargetDir(to, opts.filesMap)) {
    // copyFileSync is not atomic on non-COW filesystems: a crash mid-copy
    // can leave a partially-written file.  package.json is the completion
    // marker, so it must be written atomically via temp file + rename.
    importIndexedDir({ importFile: resilientCopyFileSync, importFileAtomic: atomicCopyFileSync }, to, opts.filesMap, opts)
    return 'copy'
  }
  return undefined
}

function atomicCopyFileSync (src: string, dest: string): void {
  const tmp = pathTemp(dest)
  try {
    resilientCopyFileSync(src, tmp)
  } catch (err) {
    try {
      fs.unlinkSync(tmp)
    } catch {} // eslint-disable-line:no-empty
    throw err
  }
  renameOverwriteSync(tmp, dest)
}
