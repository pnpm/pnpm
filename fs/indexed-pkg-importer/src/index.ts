import assert from 'assert'
import { constants, type Stats, existsSync } from 'fs'
import util from 'util'
import fs from '@pnpm/graceful-fs'
import path from 'path'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { packageImportMethodLogger } from '@pnpm/core-loggers'
import { type FilesMap, type ImportOptions, type ImportIndexedPackage } from '@pnpm/store-controller-types'
import { importIndexedDir, type ImportFile } from './importIndexedDir'

export function createIndexedPkgImporter (
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
): ImportIndexedPackage {
  const importPackage = createImportPackage(packageImportMethod)
  return importPackage
}

function createImportPackage (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy') {
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod ?? 'auto') {
  case 'clone':
    packageImportMethodLogger.debug({ method: 'clone' })
    return clonePkg.bind(null, createCloneFunction())
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
        const _clonePkg = clonePkg.bind(null, createCloneFunction())
        if (!_clonePkg(to, opts)) return undefined
        packageImportMethodLogger.debug({ method: 'clone' })
        auto = _clonePkg
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
      const _clonePkg = clonePkg.bind(null, createCloneFunction())
      if (!_clonePkg(to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'clone' })
      auto = _clonePkg
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

function clonePkg (
  clone: CloneFunction,
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (opts.resolvedFrom !== 'store' || opts.force || !existsSync(pkgJsonPath)) {
    importIndexedDir(clone, to, opts.filesMap, opts)
    return 'clone'
  }
  return undefined
}

function createCloneFunction (): CloneFunction {
  // Node.js currently does not natively support reflinks on Windows and macOS.
  // Hence, we use a third party solution.
  if (process.platform === 'darwin' || process.platform === 'win32') {
    // eslint-disable-next-line
    const { reflinkFileSync } = require('@reflink/reflink')
    return (fr, to) => {
      try {
        reflinkFileSync(fr, to)
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        // If the file already exists, then we just proceed.
        // This will probably only happen if the package's index file contains the same file twice.
        // For instance: { "index.js": "hash", "./index.js": "hash" }
        if (!err.message.startsWith('File exists') && !err.message.includes('-2147024816')) throw err
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
) {
  if (opts.force || shouldRelinkPkg(to, opts)) {
    importIndexedDir(importFile, to, opts.filesMap, opts)
    return 'hardlink'
  }
  return undefined
}

function shouldRelinkPkg (
  to: string,
  opts: ImportOptions
) {
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

function linkOrCopy (existingPath: string, newPath: string) {
  try {
    fs.linkSync(existingPath, newPath)
  } catch (err: unknown) {
    // If a hard link to the same file already exists
    // then trying to copy it will make an empty file from it.
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST') return
    // In some VERY rare cases (1 in a thousand), hard-link creation fails on Windows.
    // In that case, we just fall back to copying.
    // This issue is reproducible with "pnpm add @material-ui/icons@4.9.1"
    fs.copyFileSync(existingPath, newPath)
  }
}

function pkgLinkedToStore (
  filesMap: FilesMap,
  to: string
) {
  if (filesMap['package.json']) {
    if (isSameFile('package.json', to, filesMap)) {
      return true
    }
  } else {
    // An injected package might not have a package.json.
    // This will probably only even happen in a Bit workspace.
    const [anyFile] = Object.keys(filesMap)
    if (isSameFile(anyFile, to, filesMap)) return true
  }
  return false
}

function isSameFile (filename: string, linkedPkgDir: string, filesMap: FilesMap) {
  const linkedFile = path.join(linkedPkgDir, filename)
  let stats0!: Stats
  try {
    stats0 = fs.statSync(linkedFile)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return false
  }
  const stats1 = fs.statSync(filesMap[filename])
  if (stats0.ino === stats1.ino) return true
  globalInfo(`Relinking ${linkedPkgDir} from the store`)
  return false
}

export function copyPkg (
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (opts.resolvedFrom !== 'store' || opts.force || !existsSync(pkgJsonPath)) {
    importIndexedDir(fs.copyFileSync, to, opts.filesMap, opts)
    return 'copy'
  }
  return undefined
}
