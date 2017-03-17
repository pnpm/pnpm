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
import getNpmTarballUrl from 'get-npm-tarball-url'
import readPkg from '../fs/readPkg'
import exists = require('path-exists')
import memoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {InstallContext} from '../api/install'
import fetchResolution from './fetchResolution'
import logStatus from '../logging/logInstallStatus'
import dirsum from '../fs/dirsum'
import untouched from '../pkgIsUntouched'

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<Boolean>,
  path: string,
  srcPath?: string,
  id: string,
  resolution: Resolution,
  abort(): Promise<void>,
}

export default async function fetch (
  spec: PackageSpec,
  options: {
    root: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    update?: boolean,
    shrinkwrapResolution?: Resolution,
    pkgId?: string,
    fetchingLocker: MemoizedFunc<Boolean>,
    loggedPkg: LoggedPkg,
    offline: boolean,
  }
): Promise<FetchedPackage> {
  try {
    let fetchingPkg = null
    let resolution = options.shrinkwrapResolution
    let pkgId = options.pkgId
    if (!resolution || options.update) {
      const resolveResult = await resolve(spec, {
        loggedPkg: options.loggedPkg,
        root: options.root,
        got: options.got,
        localRegistry: options.localRegistry,
        registry: options.registry,
        metaCache: options.metaCache,
        offline: options.offline,
      })
      logStatus({status: 'resolved', pkg: options.loggedPkg})
      // keep the shrinkwrap resolution when possible
      // to keep the original shasum
      if (pkgId !== resolveResult.id || !resolution) {
        resolution = resolveResult.resolution
      }
      pkgId = resolveResult.id
      if (resolveResult.package) {
        fetchingPkg = Promise.resolve(resolveResult.package)
      }
    } else if (resolution.type === undefined && resolution.tarball === undefined) {
      const parts = pkgId!.split('/')
      resolution.tarball = getNpmTarballUrl(parts[1], parts[2], {registry: options.registry})
    }

    const id = <string>pkgId

    const target = path.join(options.storePath, id)

    const fetchingFiles = options.fetchingLocker(id, () => fetchToStore({
      target,
      resolution: <Resolution>resolution,
      loggedPkg: options.loggedPkg,
      got: options.got,
      localRegistry: options.localRegistry,
      offline: options.offline,
    }))

    if (fetchingPkg == null) {
      fetchingPkg = fetchingFiles.then(() => readPkg(target))
    }

    return {
      fetchingPkg,
      fetchingFiles,
      id,
      resolution,
      path: target,
      srcPath: resolution.type == 'directory'
        ? resolution.root
        : undefined,
      abort: async () => {
        try {
          await fetchingFiles
        } finally {
          return rimraf(target)
        }
      },
    }
  } catch (err) {
    logStatus({status: 'error', pkg: options.loggedPkg})
    throw err
  }
}

async function fetchToStore (opts: {
  target: string,
  resolution: Resolution,
  loggedPkg: LoggedPkg,
  got: Got,
  localRegistry: string,
  offline: boolean,
}): Promise<Boolean> {
  const target = opts.target
  const targetExists = await exists(target)

  if (targetExists) {
    // if target exists and it wasn't modified, then no need to refetch it
    if (await untouched(target)) return false
    logger.warn(`Refetching ${target} to store, as it was modified`)
  }

  // We fetch into targetStage directory first and then fs.rename() it to the
  // target directory.

  const targetStage = `${target}_stage`

  await rimraf(targetStage)
  if (targetExists) {
    await rimraf(target)
  }

  await fetchResolution(opts.resolution, targetStage, {
    got: opts.got,
    loggedPkg: opts.loggedPkg,
    localRegistry: opts.localRegistry,
    offline: opts.offline,
  })
  logStatus({
    status: 'fetched',
    pkg: opts.loggedPkg // TODO: add version
  })

  // fs.rename(oldPath, newPath) is an atomic operation, so we do it at the
  // end
  await fs.rename(targetStage, target)

  createShasum(target)

  return true
}

async function createShasum(dirPath: string) {
  try {
    const shasum = await dirsum(dirPath)
    await fs.writeFile(`${dirPath}_shasum`, shasum, 'utf8')
  } catch (err) {
    logger.error({
      message: `Failed to calculate shasum for ${dirPath}`,
      err,
    })
  }
}
