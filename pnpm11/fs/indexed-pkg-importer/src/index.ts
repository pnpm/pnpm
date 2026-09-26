import assert from 'node:assert'
import { chmodSync, constants, existsSync, type Stats } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { packageImportMethodLogger } from '@pnpm/core-loggers'
import fs from '@pnpm/fs.graceful-fs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import type { FilesMap, ImportIndexedPackage, ImportOptions } from '@pnpm/store.controller-types'
import { fastPathTemp as pathTemp } from 'path-temp'
import { renameOverwriteSync } from 'rename-overwrite'

import { type Importer, type ImportFile, importIndexedDir } from './importIndexedDir.js'
import { isNativeBinary, removeQuarantine } from './removeQuarantine.js'

export { type FilesMap, type ImportIndexedPackage, type ImportOptions }

const BASE_FILE_MODE = 0o666
const EXEC_FILE_MODE = 0o755
const CAFS_EXEC_SUFFIX = '-exec'
const CAFS_DIGEST_MIN_LENGTH = 40

export type PackageImportMethod = 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'

export interface CreateIndexedPkgImporterOptions {
  disableLogging?: boolean
}

export function createIndexedPkgImporter (
  packageImportMethod?: PackageImportMethod,
  opts?: CreateIndexedPkgImporterOptions
): ImportIndexedPackage {
  const importPackage = createImportPackage(packageImportMethod, opts)
  return importPackage
}

function createImportPackage (
  packageImportMethod?: PackageImportMethod,
  opts?: CreateIndexedPkgImporterOptions
): ImportIndexedPackage {
  const disableLogging = opts?.disableLogging
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod ?? 'auto') {
    case 'clone':
      if (!disableLogging) packageImportMethodLogger.debug({ method: 'clone' })
      return createClonePkg()
    case 'hardlink':
      if (!disableLogging) packageImportMethodLogger.debug({ method: 'hardlink' })
      return hardlinkPkg.bind(null, linkOrCopy)
    case 'auto': {
      return createAutoImporter(opts)
    }
    case 'clone-or-copy':
      return createCloneOrCopyImporter(opts)
    case 'copy':
      if (!disableLogging) packageImportMethodLogger.debug({ method: 'copy' })
      return copyPkg
    default:
      throw new Error(`Unknown package import method ${packageImportMethod as string}`)
  }
}

function createAutoImporter (createOpts?: CreateIndexedPkgImporterOptions): ImportIndexedPackage {
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
        if (!tryClonePkg(to, opts)) return undefined
        if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'clone' })
        auto = createClonePkg()
        return 'clone'
      } catch {
        // ignore
      }
    }
    try {
      if (!hardlinkPkg(linkOrThrowOnLinkFailure, to, opts)) return undefined
      if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return 'hardlink'
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      if (err.message.startsWith('EXDEV: cross-device link not permitted')) {
        globalWarn(err.message)
        globalInfo('Falling back to copying packages from store')
        if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'copy' })
        auto = copyPkg
        return auto(to, opts)
      }
      // We still choose hard linking that will fall back to copying in edge cases.
      if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return auto(to, opts)
    }
  }
}

function createCloneOrCopyImporter (createOpts?: CreateIndexedPkgImporterOptions): ImportIndexedPackage {
  let auto = initialAuto

  return (to, opts) => auto(to, opts)

  function initialAuto (
    to: string,
    opts: ImportOptions
  ): string | undefined {
    try {
      if (!tryClonePkg(to, opts)) return undefined
      if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'clone' })
      auto = createClonePkg()
      return 'clone'
    } catch {
      // ignore
    }
    if (!createOpts?.disableLogging) packageImportMethodLogger.debug({ method: 'copy' })
    auto = copyPkg
    return auto(to, opts)
  }
}

type CloneFunction = (src: string, dest: string) => void

function alignedClone (clone: CloneFunction): CloneFunction {
  return (src, dest) => {
    clone(src, dest)
    alignStoreFileMode(src, dest)
  }
}

/**
 * Import a single package using a raw clone function (no ENOTSUP fallback).
 * Used by auto-mode to probe whether the filesystem supports cloning.
 * If cloning isn't supported, the error propagates so the caller can fall
 * through to a faster method (e.g. hardlinks).
 */
function tryClonePkg (
  to: string,
  opts: ImportOptions
): 'clone' | undefined {
  if (shouldImportPkg(to, opts)) {
    const clone = alignedClone(createCloneFunction())
    importIndexedDir({ importFile: clone, importFileAtomic: clone }, to, opts.filesMap, opts)
    removeQuarantineFromNativeBinaries(to, opts)
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
  const clone = alignedClone(createCloneFunction())
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
    if (shouldImportPkg(to, opts)) {
      importIndexedDir(importer, to, opts.filesMap, opts)
      removeQuarantineFromNativeBinaries(to, opts)
      return 'clone'
    }
    return undefined
  }
}

// Whether to (re)import via clone or copy. A directory dependency's source
// can change without the lockfile changing, so these methods import it
// unconditionally by default, the same way `shouldRelinkPkg` does for the
// hardlink path below, including the same guard against wiping an
// already-materialized target when a `publishConfig.directory` source has
// not been built yet.
function shouldImportPkg (
  to: string,
  opts: ImportOptions
): boolean {
  if (opts.resolvedFrom === 'local-dir' && opts.sourceExists === false) {
    return targetHasNothingToLose(to)
  }
  return opts.resolvedFrom !== 'store' || opts.force || !pkgExistsAtTargetDir(to, opts.filesMap)
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

let _cloneFunction: CloneFunction | undefined

function createCloneFunction (): CloneFunction {
  if (_cloneFunction) return _cloneFunction
  // Node.js currently does not natively support reflinks on Windows and macOS.
  // Hence, we use a third party solution.
  if (process.platform === 'darwin' || process.platform === 'win32') {
    // eslint-disable-next-line
    const { reflinkFileSync } = require('@reflink/reflink') as typeof import('@reflink/reflink')
    _cloneFunction = (fr, to) => {
      try {
        reflinkFileSync(fr, to)
      } catch (err: unknown) {
        // If the file already exists, then we just proceed.
        // This will probably only happen if the package's index file contains the same file twice.
        // For instance: { "index.js": "hash", "./index.js": "hash" }
        if (!util.types.isNativeError(err) || !('code' in err) || err.code !== 'EEXIST') throw err
      }
    }
  } else {
    _cloneFunction = (src: string, dest: string) => {
      try {
        fs.copyFileSync(src, dest, constants.COPYFILE_FICLONE_FORCE)
      } catch (err: unknown) {
        if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
      }
    }
  }
  return _cloneFunction
}

function hardlinkPkg (
  importFile: ImportFile,
  to: string,
  opts: ImportOptions
): 'hardlink' | undefined {
  // Checked ahead of `opts.force`: a caller-requested force must not be able
  // to bypass the missing-source preservation below.
  if (missingSourceHasSomethingToPreserve(to, opts)) return undefined
  if (opts.force || shouldRelinkPkg(to, opts)) {
    importIndexedDir({ importFile, importFileAtomic: importFile }, to, opts.filesMap, opts)
    removeQuarantineFromNativeBinaries(to, opts)
    return 'hardlink'
  }
  return undefined
}

function shouldRelinkPkg (
  to: string,
  opts: ImportOptions
): boolean {
  if (opts.disableRelinkLocalDirDeps && opts.resolvedFrom === 'local-dir') {
    return targetHasNothingToLose(to)
  }
  // A directory dependency relinks on every install by default, since its
  // source can change without the lockfile changing. But when the source is
  // a `publishConfig.directory` its own `prepare` script has not (re)built
  // yet, the fetcher tolerates the missing directory and comes back with an
  // empty `filesMap`; relinking from that would wipe an already-materialized
  // target. Only skip the relink when there is something in the target
  // worth preserving — an empty or missing target has nothing to lose, and
  // still needs the relink to create it so the post-build resync has
  // somewhere to write into. `hardlinkPkg` checks this same condition ahead
  // of its own `opts.force`, so this only runs once that has already passed.
  if (opts.resolvedFrom === 'local-dir' && opts.sourceExists === false) {
    return targetHasNothingToLose(to)
  }
  return opts.resolvedFrom !== 'store' || !pkgLinkedToStore(opts.filesMap, to)
}

// See `shouldRelinkPkg`'s comment on the missing-source case. Pulled out so
// `hardlinkPkg` can apply the same guard ahead of its `opts.force` check,
// which would otherwise bypass it.
function missingSourceHasSomethingToPreserve (to: string, opts: ImportOptions): boolean {
  return opts.resolvedFrom === 'local-dir' && opts.sourceExists === false && !targetHasNothingToLose(to)
}

function targetHasNothingToLose (to: string): boolean {
  try {
    const files = fs.readdirSync(to)
    return files.length === 0 || files.length === 1 && files[0] === 'node_modules'
  } catch {
    return true
  }
}

// The CLI never changes its own umask and the import path asks for it once
// per file, so read it once.
let umask: number | undefined

function storeEntryMode (executable: boolean): number {
  umask ??= process.umask()
  return ((executable ? EXEC_FILE_MODE : BASE_FILE_MODE) & ~umask) & 0o777
}

// The exact inverse of the CAFS layout `<store>/v<N>/files/<2 hex digits>/<digest>[-exec]`.
// This importer also materializes local-directory dependencies, whose files keep
// the modes their project gives them, so only a real store entry is re-derived.
function isCafsFile (src: string): boolean {
  const nameSep = src.lastIndexOf(path.sep)
  if (nameSep < 0) return false
  const shardSep = src.lastIndexOf(path.sep, nameSep - 1)
  if (shardSep < 0 || nameSep - shardSep - 1 !== 2 || !isLowerHex(src, shardSep + 1, nameSep)) return false
  const filesSep = src.lastIndexOf(path.sep, shardSep - 1)
  if (filesSep < 0 || shardSep - filesSep - 1 !== 'files'.length || !src.startsWith('files', filesSep + 1)) return false
  const digestEnd = src.endsWith(CAFS_EXEC_SUFFIX) ? src.length - CAFS_EXEC_SUFFIX.length : src.length
  return digestEnd - nameSep - 1 >= CAFS_DIGEST_MIN_LENGTH && isLowerHex(src, nameSep + 1, digestEnd)
}

function isLowerHex (value: string, from: number, to: number): boolean {
  for (let i = from; i < to; i++) {
    const code = value.charCodeAt(i)
    if (!((code >= 0x30 && code <= 0x39) || (code >= 0x61 && code <= 0x66))) return false
  }
  return true
}

// A store file carries the umask that was active when it was written, so
// importing has to apply the current one as though the entry were just
// written (pnpm/pnpm#3807). POSIX mode bits do not apply on Windows.
function storeEntryModeForSource (src: string): number | undefined {
  if (process.platform === 'win32') return undefined
  return isCafsFile(src) ? storeEntryMode(src.endsWith(CAFS_EXEC_SUFFIX)) : undefined
}

function alignStoreFileMode (src: string, dest: string): void {
  const mode = storeEntryModeForSource(src)
  if (mode === undefined) return
  chmodSync(dest, mode)
}

function storeModeDiffers (existingPath: string): boolean {
  const storeMode = storeEntryModeForSource(existingPath)
  return storeMode !== undefined && (fs.statSync(existingPath).mode & 0o777) !== storeMode
}

function linkOrCopy (existingPath: string, newPath: string): void {
  // A hardlink would pin the store-population mode onto the import.
  if (storeModeDiffers(existingPath)) {
    resilientCopyFileSync(existingPath, newPath)
    return
  }
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

// The auto-mode hardlink probe: a filesystem that cannot hardlink must keep
// failing here, so auto backs off to copy.
function linkOrThrowOnLinkFailure (existingPath: string, newPath: string): void {
  if (storeModeDiffers(existingPath)) {
    resilientCopyFileSync(existingPath, newPath)
    return
  }
  fs.linkSync(existingPath, newPath)
}

// On Linux CI, the kernel's copy_file_range/sendfile can transiently fail
// with ENOTSUP under heavy parallel I/O on the same store files.
// Fall back to manual read+write which uses plain read/write syscalls.
function resilientCopyFileSync (src: string, dest: string): void {
  try {
    fs.copyFileSync(src, dest)
    alignStoreFileMode(src, dest)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOTSUP') {
      const storeMode = storeEntryModeForSource(src)
      const srcMode = storeMode ?? fs.statSync(src).mode
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
  if (shouldImportPkg(to, opts)) {
    // copyFileSync is not atomic on non-COW filesystems: a crash mid-copy
    // can leave a partially-written file.  package.json is the completion
    // marker, so it must be written atomically via temp file + rename.
    importIndexedDir({ importFile: resilientCopyFileSync, importFileAtomic: atomicCopyFileSync }, to, opts.filesMap, opts)
    removeQuarantineFromNativeBinaries(to, opts)
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

// macOS preserves the com.apple.quarantine xattr when files are copied or
// reflinked out of the store, and for hardlinks the imported file shares the
// store blob's inode (and thus its xattrs). Either way Gatekeeper can block a
// native binary from loading. Drop the quarantine once, in a single batched
// `xattr` call per package, restricted to the few native binaries that
// Gatekeeper actually guards. Only store imports are cleaned: that's where the
// quarantine propagation happens and where pnpm has verified file integrity.
function removeQuarantineFromNativeBinaries (to: string, opts: ImportOptions): void {
  if (process.platform !== 'darwin' || opts.resolvedFrom !== 'store') return
  const nativeBinaries: string[] = []
  for (const file of opts.filesMap.keys()) {
    if (isNativeBinary(file)) {
      nativeBinaries.push(path.join(to, file))
    }
  }
  removeQuarantine(nativeBinaries)
}
