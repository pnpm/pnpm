import { importingLogger } from '@pnpm/core-loggers'
import { storeLogger } from '@pnpm/logger'
import {
  ImportPackageFunction,
  PackageFilesResponse,
} from '@pnpm/store-controller-types'
import child_process = require('child_process')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import ncpCB = require('ncp')
import pLimit from 'p-limit'
import path = require('path')
import exists = require('path-exists')
import pathTemp = require('path-temp')
import renameOverwrite = require('rename-overwrite')
import { promisify } from 'util'
import linkIndexedDir from '../fs/linkIndexedDir'

const execFilePromise = promisify(child_process.execFile)
const ncp = promisify(ncpCB)
const limitLinking = pLimit(16)

export default (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink'): ImportPackageFunction => {
  const importPackage = createImportPackage(packageImportMethod)
  return (filesResponse, dependency, opts) => limitLinking(() => importPackage(filesResponse, dependency, opts))
}

function createImportPackage (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink') {
  let fallbackToCopying = false

  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - reflink: reflink the packages, no fallback
  // - auto: try to hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod || 'auto') {
    case 'reflink':
      return reflinkPkg
    case 'hardlink':
      return hardlinkPkg
    case 'auto':
      return async function importPackage (
        from: string,
        to: string,
        opts: {
          filesResponse: PackageFilesResponse,
          force: boolean,
        }) {
        if (fallbackToCopying) {
          await copyPkg(from, to, opts)
          return
        }
        try {
          await hardlinkPkg(from, to, opts)
        } catch (err) {
          if (!err.message.startsWith('EXDEV: cross-device link not permitted')) throw err
          storeLogger.warn(err.message)
          storeLogger.info('Falling back to copying packages from store')
          fallbackToCopying = true
          await importPackage(from, to, opts)
        }
      }
    case 'copy':
      return copyPkg
    default:
      throw new Error(`Unknown package import method ${packageImportMethod}`)
  }
}

async function reflinkPkg (
  from: string,
  to: string,
  opts: {
    filesResponse: PackageFilesResponse,
    force: boolean,
  },
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.filesResponse.fromStore || opts.force || !await exists(pkgJsonPath)) {
    importingLogger.debug({ from, to, method: 'reflink' })
    const staging = pathTemp(path.dirname(to))
    await makeDir(staging)
    await execFilePromise('cp', ['-r', '--reflink', from + '/.', staging])
    await renameOverwrite(staging, to)
  }
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
    await linkIndexedDir(from, to, opts.filesResponse.filenames)
  }
}

async function pkgLinkedToStore (
  pkgJsonPath: string,
  from: string,
  to: string,
) {
  const pkgJsonPathInStore = path.join(from, 'package.json')
  if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
  storeLogger.info(`Relinking ${to} from the store`)
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
    await makeDir(staging)
    await ncp(from + '/.', staging)
    await renameOverwrite(staging, to)
  }
}
