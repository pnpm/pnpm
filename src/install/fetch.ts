import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {Resolution, PackageSpec} from '../resolve'
import mkdirp from '../fs/mkdirp'
import readPkg from '../fs/readPkg'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import memoize, {MemoizedFunc} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {InstallContext} from '../api/install'
import fetchResolution from './fetchResolution'
import logStatus from '../logging/logInstallStatus'
import {PackageMeta} from '../resolve/utils/loadPackageMeta'
import dirsum from '../fs/dirsum'
import untouched from '../pkgIsUntouched'

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<Boolean>,
  path: string,
  srcPath?: string,
  id: string,
  fromCache: boolean,
  abort(): Promise<void>,
}

export default async function fetch (
  ctx: InstallContext,
  spec: PackageSpec,
  modules: string,
  options: {
    linkLocal: boolean,
    force: boolean,
    root: string,
    storePath: string,
    metaCache: Map<string, PackageMeta>,
    tag: string,
    got: Got,
    update?: boolean,
    shrinkwrapResolution?: Resolution,
    pkgId?: string,
  }
): Promise<FetchedPackage> {
  logger.debug('installing ' + spec.raw)

  const loggedPkg: LoggedPkg = {
    rawSpec: spec.rawSpec,
    name: spec.name,
  }

  try {
    let fetchingPkg = null
    let resolution = options.shrinkwrapResolution
    let pkgId = options.pkgId
    if (!resolution && !options.force) {
      // it might be a bundleDependency, in which case, don't bother
      if (await isAvailable(spec, modules)) {
        return await saveCachedResolution()
      }
    }
    if (!resolution || options.update) {
      const resolveResult = await resolve(spec, {
        loggedPkg,
        root: options.root,
        got: options.got,
        tag: options.tag,
        storePath: options.storePath,
        metaCache: options.metaCache,
      })
      resolution = resolveResult.resolution
      pkgId = resolveResult.id
      if (resolveResult.package) {
        fetchingPkg = Promise.resolve(resolveResult.package)
      }
      ctx.shrinkwrap.packages[resolveResult.id] = {resolution}
    }

    const id = <string>pkgId

    const target = path.join(options.storePath, id)

    const fetchingFiles = ctx.fetchingLocker(id, () => fetchToStore({
      target,
      resolution: <Resolution>resolution,
      loggedPkg,
      got: options.got,
      linkLocal: options.linkLocal,
    }))

    if (fetchingPkg == null) {
      fetchingPkg = fetchingFiles.then(() => readPkg(target))
    }

    return {
      fetchingPkg,
      fetchingFiles,
      id,
      fromCache: false,
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
    logStatus({status: 'error', pkg: loggedPkg})
    throw err
  }

  async function saveCachedResolution (): Promise<FetchedPackage> {
    const target = path.join(modules, spec.name)
    const stat: Stats = await fs.lstat(target)
    if (stat.isSymbolicLink()) {
      const linkPath = await fs.readlink(target)
      return save(path.resolve(linkPath, target))
    }
    return save(target)

    async function save (fullpath: string): Promise<FetchedPackage> {
      return {
        fetchingPkg: readPkg(fullpath),
        fetchingFiles: Promise.resolve(false), // this property can be ignored by cached packages at all
        id: path.basename(fullpath),
        fromCache: true,
        path: fullpath,
        abort: () => Promise.resolve(),
      }
    }
  }
}

async function fetchToStore (opts: {
  target: string,
  resolution: Resolution,
  loggedPkg: LoggedPkg,
  got: Got,
  linkLocal: boolean,
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

  logStatus({status: 'download-queued', pkg: opts.loggedPkg})
  await fetchResolution(opts.resolution, targetStage, {
    got: opts.got,
    loggedPkg: opts.loggedPkg,
    linkLocal: opts.linkLocal,
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
