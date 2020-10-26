import { packageImportMethodLogger } from '@pnpm/core-loggers'
import { globalInfo, globalWarn } from '@pnpm/logger'
import importIndexedDir, { ImportFile } from '../fs/importIndexedDir'
import path = require('path')
import fs = require('mz/fs')
import pLimit = require('p-limit')
import exists = require('path-exists')

const limitLinking = pLimit(16)

interface ImportOptions {
  filesMap: Record<string, string>
  force: boolean
  fromStore: boolean
}

type ImportFunction = (to: string, opts: ImportOptions) => Promise<string | undefined>

export default (
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
): ImportFunction => {
  const importPackage = createImportPackage(packageImportMethod)
  return (to, opts) => limitLinking(() => importPackage(to, opts))
}

function createImportPackage (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone') {
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod ?? 'auto') {
  case 'clone':
    packageImportMethodLogger.debug({ method: 'clone' })
    return clonePkg
  case 'hardlink':
    packageImportMethodLogger.debug({ method: 'hardlink' })
    return hardlinkPkg.bind(null, linkOrCopy)
  case 'auto': {
    return createAutoImporter()
  }
  case 'copy':
    packageImportMethodLogger.debug({ method: 'copy' })
    return copyPkg
  default:
    throw new Error(`Unknown package import method ${packageImportMethod as string}`)
  }
}

function createAutoImporter (): ImportFunction {
  let auto = initialAuto

  return (to, opts) => auto(to, opts)

  async function initialAuto (
    to: string,
    opts: ImportOptions
  ): Promise<string | undefined> {
    try {
      if (!await clonePkg(to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'clone' })
      auto = clonePkg
      return 'clone'
    } catch (err) {
      // ignore
    }
    try {
      if (!await hardlinkPkg(fs.link, to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return 'hardlink'
    } catch (err) {
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

async function clonePkg (
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    await importIndexedDir(cloneFile, to, opts.filesMap)
    return 'clone'
  }
  return undefined
}

async function cloneFile (from: string, to: string) {
  await fs.copyFile(from, to, fs.constants.COPYFILE_FICLONE_FORCE)
}

async function hardlinkPkg (
  importFile: ImportFile,
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore(pkgJsonPath, opts.filesMap['package.json'], to)) {
    await importIndexedDir(importFile, to, opts.filesMap)
    return 'hardlink'
  }
  return undefined
}

async function linkOrCopy (existingPath: string, newPath: string) {
  try {
    await fs.link(existingPath, newPath)
  } catch (err) {
    // If a hard link to the same file already exists
    // then trying to copy it will make an empty file from it.
    if (err['code'] === 'EEXIST') return
    // In some VERY rare cases (1 in a thousand), hard-link creation fails on Windows.
    // In that case, we just fall back to copying.
    // This issue is reproducible with "pnpm add @material-ui/icons@4.9.1"
    await fs.copyFile(existingPath, newPath)
  }
}

async function pkgLinkedToStore (
  pkgJsonPath: string,
  pkgJsonPathInStore: string,
  to: string
) {
  if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
  globalInfo(`Relinking ${to} from the store`)
  return false
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
}

export async function copyPkg (
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    await importIndexedDir(fs.copyFile, to, opts.filesMap)
    return 'copy'
  }
  return undefined
}
