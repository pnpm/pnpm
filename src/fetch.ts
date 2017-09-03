import logger, {
  LoggedPkg,
  progressLogger,
} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {
  Resolution,
  DirectoryResolution,
  PackageSpec,
  PackageMeta,
} from './resolve'
import mkdirp = require('mkdirp-promise')
import writeJsonFile = require('write-json-file')
import pkgIdToFilename from './fs/pkgIdToFilename'
import {fromDir as readPkgFromDir} from './fs/readPkg'
import {fromDir as safeReadPkgFromDir} from './fs/safeReadPkg'
import {Store} from './fs/storeController'
import exists = require('path-exists')
import memoize, {MemoizedFunc} from './memoize'
import {Package} from './types'
import {Got} from './network/got'
import fetchResolution from './fetchResolution'
import untouched from './pkgIsUntouched'
import symlinkDir = require('symlink-dir')
import * as unpackStream from 'unpack-stream'
import renameOverwrite = require('rename-overwrite')
import loadJsonFile = require('load-json-file')

export type PackageContentInfo = {
  isNew: boolean,
  index: {},
}

export type FetchedPackage = {
  isLocal: true,
  resolution: DirectoryResolution,
  pkg: Package,
  id: string,
} | {
  isLocal: false,
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<PackageContentInfo>,
  calculatingIntegrity: Promise<void>,
  path: string,
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
        calculatingIntegrity: Promise<void>,
      },
    },
    loggedPkg: LoggedPkg,
    offline: boolean,
    storeIndex: Store,
    downloadPriority: number,
    verifyStoreIntegrity: boolean,
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
        downloadPriority: options.downloadPriority,
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

    progressLogger.debug({status: 'resolved', pkgId: id, pkg: options.loggedPkg})

    if (resolution.type === 'directory') {
      if (!pkg) {
        throw new Error(`Couldn't read package.json of local dependency ${spec}`)
      }
      return {
        isLocal: true,
        id,
        pkg,
        resolution,
      }
    }

    const targetRelative = pkgIdToFilename(id)
    const target = path.join(options.storePath, targetRelative)

    if (!options.fetchingLocker[id]) {
      options.fetchingLocker[id] = fetchToStore({
        target,
        targetRelative,
        resolution: <Resolution>resolution,
        pkgId: id,
        got: options.got,
        storePath: options.storePath,
        offline: options.offline,
        pkg,
        storeIndex: options.storeIndex,
        verifyStoreIntegrity: options.verifyStoreIntegrity,
        prefix: options.prefix,
      })
    }

    return {
      isLocal: false,
      fetchingPkg: options.fetchingLocker[id].fetchingPkg,
      fetchingFiles: options.fetchingLocker[id].fetchingFiles,
      calculatingIntegrity: options.fetchingLocker[id].calculatingIntegrity,
      id,
      resolution,
      path: target,
    }
  } catch (err) {
    progressLogger.debug({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

function fetchToStore (opts: {
  target: string,
  targetRelative: string,
  resolution: Resolution,
  pkgId: string,
  got: Got,
  storePath: string,
  offline: boolean,
  pkg?: Package,
  storeIndex: Store,
  verifyStoreIntegrity: boolean,
  prefix: string,
}): {
  fetchingFiles: Promise<PackageContentInfo>,
  fetchingPkg: Promise<Package>,
  calculatingIntegrity: Promise<void>,
} {
  const fetchingPkg = differed<Package>()
  const fetchingFiles = differed<PackageContentInfo>()
  const calculatingIntegrity = differed<void>()

  fetch()

  return {
    fetchingFiles: fetchingFiles.promise,
    fetchingPkg: opts.pkg && Promise.resolve(opts.pkg) || fetchingPkg.promise,
    calculatingIntegrity: calculatingIntegrity.promise,
  }

  async function fetch () {
    try {
      progressLogger.debug({
        status: 'resolving_content',
        pkgId: opts.pkgId,
      })

      let target = opts.target
      const linkToUnpacked = path.join(target, 'package')

      // We can safely assume that if there is no data about the package in `store.json` then
      // it is not in the store yet.
      // In case there is record about the package in `store.json`, we check it in the file system just in case
      const targetExists = opts.storeIndex[opts.targetRelative] && await exists(path.join(linkToUnpacked, 'package.json'))

      if (targetExists) {
        // if target exists and it wasn't modified, then no need to refetch it
        const satisfiedIntegrity = opts.verifyStoreIntegrity
          ? await untouched(linkToUnpacked)
          : await loadJsonFile(path.join(path.dirname(linkToUnpacked), 'integrity.json'))
        if (satisfiedIntegrity) {
          progressLogger.debug({
            status: 'found_in_store',
            pkgId: opts.pkgId,
          })
          fetchingFiles.resolve({
            isNew: false,
            index: satisfiedIntegrity,
          })
          if (!opts.pkg) {
            readPkgFromDir(linkToUnpacked)
              .then(pkg => fetchingPkg.resolve(pkg))
              .catch(err => fetchingPkg.reject(err))
          }
          calculatingIntegrity.resolve(undefined)
          return
        }
        logger.warn(`Refetching ${target} to store, as it was modified`)
      }

      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      const targetStage = `${target}_stage`

      await rimraf(targetStage)

      let packageIndex: {} = {}
      await Promise.all([
        async function () {
          packageIndex = await fetchResolution(opts.resolution, targetStage, {
            got: opts.got,
            pkgId: opts.pkgId,
            storePath: opts.storePath,
            offline: opts.offline,
            prefix: opts.prefix,
          })
        }(),
        // removing only the folder with the unpacked files
        // not touching tarball and integrity.json
        targetExists && await rimraf(path.join(target, 'node_modules'))
      ])
      progressLogger.debug({
        status: 'fetched',
        pkgId: opts.pkgId,
      })

      // fetchingFilse shouldn't care about when this is saved at all
      if (!targetExists) {
        (async function () {
          const integrity = opts.verifyStoreIntegrity
            ? await (<unpackStream.Index>packageIndex).integrityPromise
            : await (<unpackStream.Index>packageIndex).headers
          writeJsonFile(path.join(target, 'integrity.json'), integrity, {indent: null})
          calculatingIntegrity.resolve(undefined)
        })()
      } else {
        calculatingIntegrity.resolve(undefined)
      }

      let pkg: Package
      if (opts.pkg) {
        pkg = opts.pkg
      } else {
        pkg = await readPkgFromDir(targetStage)
        fetchingPkg.resolve(pkg)
      }

      const unpacked = path.join(target, 'node_modules', pkg.name)
      await mkdirp(path.dirname(unpacked))

      // rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await renameOverwrite(targetStage, unpacked)
      await symlinkDir(unpacked, linkToUnpacked)

      fetchingFiles.resolve({
        isNew: true,
        index: (<unpackStream.Index>packageIndex).headers,
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
  const promise = new Promise<T>((resolve, reject) => {
    pResolve = resolve
    pReject = reject
  })
  return {
    promise,
    resolve: pResolve,
    reject: pReject,
  }
}
