import { importingLogger } from '@pnpm/core-loggers'
import { globalInfo, globalWarn } from '@pnpm/logger'
import {
  ImportPackageFunction,
  PackageFilesResponse,
} from '@pnpm/store-controller-types'
import fs = require('mz/fs')
import ncpCB = require('ncp')
import pLimit from 'p-limit'
import path = require('path')
import exists = require('path-exists')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')
import { promisify } from 'util'
import importIndexedDir from '../fs/importIndexedDir'

const ncp = promisify(ncpCB)
const limitLinking = pLimit(16)

export default (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone'): ImportPackageFunction => {
  const importPackage = createImportPackage(packageImportMethod)
  return (from, to, opts) => limitLinking(() => importPackage(from, to, opts))
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
    from: string,
    to: string,
    opts: {
      filesResponse: PackageFilesResponse,
      force: boolean,
    },
  ) {
    try {
      await clonePkg(from, to, opts)
      auto = clonePkg
      return
    } catch (err) {
      // ignore
    }
    try {
      await hardlinkPkg(from, to, opts)
      auto = hardlinkPkg
      return
    } catch (err) {
      if (!err.message.startsWith('EXDEV: cross-device link not permitted')) throw err
      globalWarn(err.message)
      globalInfo('Falling back to copying packages from store')
      auto = copyPkg
      await auto(from, to, opts)
    }
  }
}

async function clonePkg (
  from: string,
  to: string,
  opts: {
    filesResponse: PackageFilesResponse,
    force: boolean,
  },
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.filesResponse.fromStore || opts.force || !await exists(pkgJsonPath)) {
    importingLogger.debug({ from, to, method: 'clone' })
    await importIndexedDir(cloneFile, from, to, opts.filesResponse.filenames)
  }
}

async function cloneFile (from: string, to: string) {
  await fs.copyFile(from, to, fs.constants.COPYFILE_FICLONE_FORCE)
}

async function hardlinkPkg (
  from: string,
  to: string,
  opts: {
    filesResponse: PackageFilesResponse,
    force: boolean,
  },
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.filesResponse.fromStore || opts.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore(pkgJsonPath, from, to)) {
    importingLogger.debug({ from, to, method: 'hardlink' })
    await importIndexedDir(fs.link, from, to, opts.filesResponse.filenames)
  }
}

async function pkgLinkedToStore (
  pkgJsonPath: string,
  from: string,
  to: string,
) {
  const pkgJsonPathInStore = path.join(from, 'package.json')
  if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
  globalInfo(`Relinking ${to} from the store`)
  return false
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
}

export async function copyPkg (
  from: string,
  to: string,
  opts: {
    filesResponse: PackageFilesResponse,
    force: boolean,
  },
) {
  const pkgJsonPath = path.join(to, 'package.json')
  if (!opts.filesResponse.fromStore || opts.force || !await exists(pkgJsonPath)) {
    importingLogger.debug({ from, to, method: 'copy' })
    const staging = pathTemp(path.dirname(to))
    await fs.mkdir(staging, { recursive: true })
    await ncp(from + '/.', staging)
    await renameOverwrite(staging, to)
  }
}
