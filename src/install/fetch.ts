import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {
  Resolution,
  PackageSpec,
  PackageMeta,
} from '../resolve'
import mkdirp = require('mkdirp-promise')
import writeJsonFile = require('write-json-file')
import pkgIdToFilename from '../fs/pkgIdToFilename'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import {fromDir as safeReadPkgFromDir} from '../fs/safeReadPkg'
import exists = require('path-exists')
import memoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {InstallContext, PackageContentInfo} from '../api/install'
import fetchResolution from './fetchResolution'
import logStatus from '../logging/logInstallStatus'
import untouched from '../pkgIsUntouched'
import symlinkDir = require('symlink-dir')

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<{
    isNew: boolean,
    index: {},
  }>,
  path: string,
  srcPath?: string,
  id: string,
  resolution: Resolution,
}

export default async function fetch (
  spec: PackageSpec,
  options: {
    prefix: string,
    storePath: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    update?: boolean,
    shrinkwrapResolution?: Resolution,
    pkgId?: string,
    fetchingLocker: {
      [pkgId: string]: {
        fetchingFiles: Promise<PackageContentInfo>,
        fetchingPkg: Promise<Package>,
      },
    },
    loggedPkg: LoggedPkg,
    offline: boolean,
  }
): Promise<FetchedPackage> {
  try {
    let pkg: Package | undefined = undefined
    let resolution = options.shrinkwrapResolution
    let pkgId = options.pkgId
    if (!resolution || options.update) {
      const resolveResult = await resolve(spec, {
        loggedPkg: options.loggedPkg,
        prefix: options.prefix,
        got: options.got,
        storePath: options.storePath,
        registry: options.registry,
        metaCache: options.metaCache,
        offline: options.offline,
      })
      // keep the shrinkwrap resolution when possible
      // to keep the original shasum
      if (pkgId !== resolveResult.id || !resolution) {
        resolution = resolveResult.resolution
      }
      pkgId = resolveResult.id
      pkg = resolveResult.package
    }
    const id = <string>pkgId

    logStatus({status: 'resolved', pkgId: id, pkg: options.loggedPkg})

    const target = path.join(options.storePath, pkgIdToFilename(id))

    if (!options.fetchingLocker[id]) {
      options.fetchingLocker[id] = fetchToStore({
        target,
        resolution: <Resolution>resolution,
        pkgId: id,
        got: options.got,
        storePath: options.storePath,
        offline: options.offline,
        pkg,
      })
    }

    return {
      fetchingPkg: options.fetchingLocker[id].fetchingPkg,
      fetchingFiles: options.fetchingLocker[id].fetchingFiles,
      id,
      resolution,
      path: target,
      srcPath: resolution.type == 'directory'
        ? path.join(options.prefix, resolution.directory)
        : undefined,
    }
  } catch (err) {
    logStatus({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

function fetchToStore (opts: {
  target: string,
  resolution: Resolution,
  pkgId: string,
  got: Got,
  storePath: string,
  offline: boolean,
  pkg?: Package,
}): {
  fetchingFiles: Promise<PackageContentInfo>,
  fetchingPkg: Promise<Package>,
} {
  const fetchingPkg = differed<Package>()
  const fetchingFiles = differed<PackageContentInfo>()

  fetch()

  return {
    fetchingFiles: fetchingFiles.promise,
    fetchingPkg: opts.pkg && Promise.resolve(opts.pkg) || fetchingPkg.promise,
  }

  async function fetch () {
    try {
      let target = opts.target
      const linkToUnpacked = path.join(target, 'package')
      const targetExists = await exists(path.join(linkToUnpacked, 'package.json'))

      if (targetExists) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = await untouched(linkToUnpacked)
        if (satisfiedIntegrity) {
          fetchingFiles.resolve({
            isNew: false,
            index: satisfiedIntegrity,
          })
          if (!opts.pkg) {
            readPkgFromDir(linkToUnpacked)
              .then(pkg => fetchingPkg.resolve(pkg))
              .catch(err => fetchingPkg.reject(err))
          }
          return
        }
        logger.warn(`Refetching ${target} to store, as it was modified`)
      }

      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      const targetStage = `${target}_stage`

      await rimraf(targetStage)
      if (targetExists) {
        await rimraf(target)
      }

      const dirIntegrity = await fetchResolution(opts.resolution, targetStage, {
        got: opts.got,
        pkgId: opts.pkgId,
        storePath: opts.storePath,
        offline: opts.offline,
      })
      logStatus({
        status: 'fetched',
        pkgId: opts.pkgId,
      })

      let pkg: Package
      if (opts.pkg) {
        pkg = opts.pkg
      } else {
        pkg = await readPkgFromDir(targetStage)
        fetchingPkg.resolve(pkg)
      }
      const unpacked = path.join(target, 'node_modules', pkg.name)
      await writeJsonFile(path.join(target, 'integrity.json'), dirIntegrity)
      await mkdirp(path.dirname(unpacked))

      // fs.rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await fs.rename(targetStage, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      fetchingFiles.resolve({
        isNew: true,
        index: dirIntegrity,
      })
    } catch (err) {
      fetchingFiles.reject(err)
      if (!opts.pkg) {
        fetchingPkg.reject(err)
      }
    }
  }
}

function differed<T> (): {
  promise: Promise<T>,
  resolve: (v: T) => void,
  reject: (err: Error) => void,
} {
  let pResolve: (v: T) => void = () => {}
  let pReject: (err: Error) => void = () => {}
  const promise = new Promise((resolve, reject) => {
    pResolve = resolve
    pReject = reject
  })
  return {
    promise,
    resolve: pResolve,
    reject: pReject,
  }
}
