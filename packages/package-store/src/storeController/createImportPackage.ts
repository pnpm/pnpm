import { importingLogger } from '@pnpm/core-loggers'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { PackageFilesResponse } from '@pnpm/store-controller-types'
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import exists = require('path-exists')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')
import importIndexedDir from '../fs/importIndexedDir'

const limitLinking = pLimit(16)

export default (
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'
): (
  to: string,
  opts: {
    filesMap: Record<string, string>,
    fromStore: boolean,
    force: boolean
  }
) => ReturnType<(to: string, opts: { filesResponse: PackageFilesResponse, force: boolean }) => Promise<void>> => {
  const importPackage = createImportPackage(packageImportMethod)
  return (to, opts) => limitLinking(() => importPackage(to, opts))
}

function createImportPackage (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone') {
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod || 'auto') {
    case 'clone':
      return clonePkg
    case 'hardlink':
      return hardlinkPkg
    case 'auto': {
      return createAutoImporter()
    }
    case 'copy':
      return copyPkg
    default:
      throw new Error(`Unknown package import method ${packageImportMethod}`)
  }
}

function createAutoImporter () {
  let auto = initialAuto

  return auto

  async function initialAuto (
    to: string,
    opts: {
      filesMap: Record<string, string>,
      force: boolean,
      fromStore: boolean,
    }
  ) {
    try {
      await clonePkg(to, opts)
      auto = clonePkg
      return
    } catch (err) {
      // ignore
    }
    try {
      await hardlinkPkg(to, opts)
      auto = hardlinkPkg
      return
    } catch (err) {
      if (!err.message.startsWith('EXDEV: cross-device link not permitted')) throw err
      globalWarn(err.message)
      globalInfo('Falling back to copying packages from store')
      auto = copyPkg
      await auto(to, opts)
    }
  }
}

async function clonePkg (
  to: string,
  opts: {
    filesMap: Record<string, string>,
    fromStore: boolean,
    force: boolean,
  }
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    importingLogger.debug({ to, method: 'clone' })
    await importIndexedDir(cloneFile, to, opts.filesMap)
  }
}

async function cloneFile (from: string, to: string) {
  await fs.copyFile(from, to, fs.constants.COPYFILE_FICLONE_FORCE)
}

async function hardlinkPkg (
  to: string,
  opts: {
    filesMap: Record<string, string>,
    force: boolean,
    fromStore: boolean,
  }
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore(pkgJsonPath, opts.filesMap['package.json'], to)) {
    importingLogger.debug({ to, method: 'hardlink' })
    await importIndexedDir(fs.link, to, opts.filesMap)
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
  opts: {
    filesMap: Record<string, string>,
    fromStore: boolean,
    force: boolean,
  }
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    importingLogger.debug({ to, method: 'copy' })
    await importIndexedDir(fs.copyFile, to, opts.filesMap)
  }
}
