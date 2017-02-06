import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {Resolution, PackageSpec} from '../resolve'
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import * as Shrinkwrap from '../fs/shrinkwrap'
import memoize, {CachedPromises} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {InstallContext} from '../api/install'
import fetchResolution from './fetchResolution'
import logStatus from '../logging/logInstallStatus'
import {PackageMeta} from '../resolve/utils/loadPackageMeta'
import dirsum from '../fs/dirsum'
import untouched from '../pkgIsUntouched'

export type FetchOptions = {
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  metaCache: Map<string, PackageMeta>,
  tag: string,
  got: Got,
  update?: boolean,
}

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<Boolean>,
  path: string,
  srcPath?: string,
  id: string,
  fromCache: boolean,
  abort(): Promise<void>,
}

/**
 * Installs a package.
 *
 * What it does:
 *
 * - resolve() - resolve from registry.npmjs.org
 * - fetch() - download tarball into node_modules/.store/{name}@{version}
 * - recurse into its dependencies
 * - symlink node_modules/{name}
 *
 * @param {Object} ctx - the context.
 * @param {Object} pkgMeta - meta info about the package to install.
 *
 * @example
 *     install(ctx, 'rimraf@2', './node_modules')
 */
export default async function fetch (ctx: InstallContext, spec: PackageSpec, modules: string, options: FetchOptions): Promise<FetchedPackage> {
  logger.debug('installing ' + spec.raw)

  const loggedPkg: LoggedPkg = {
    rawSpec: spec.rawSpec,
    name: spec.name,
  }

  try {
    let fetchingPkg = null
    let resolution = Shrinkwrap.lookupResolution(ctx.shrinkwrap, spec.raw)
    if (!resolution) {
      // it might be a bundleDependency, in which case, don't bother
      const available = !options.force && await isAvailable(spec, modules)
      if (available) {
        const fetchedPkg = await saveCachedResolution()
        return fetchedPkg
      }
    }
    if (!resolution || options.update) {
      let resolveResult = await resolve(spec, {
        loggedPkg,
        got: options.got,
        root: options.root,
        tag: options.tag,
        storePath: options.storePath,
        metaCache: options.metaCache,
      })
      resolution = resolveResult.resolution
      if (resolveResult.package) {
        fetchingPkg = Promise.resolve(resolveResult.package)
      }
      Shrinkwrap.putResolution(ctx.shrinkwrap, spec.raw, resolution)
    }

    const target = path.join(options.storePath, resolution.id)

    const fetchingFiles = fetchToStoreCached({
      fetchLocks: ctx.fetchLocks,
      target,
      resolution,
      loggedPkg,
      got: options.got,
      linkLocal: options.linkLocal,
      limitFetch: ctx.limitFetch,
    })

    if (fetchingPkg == null) {
      fetchingPkg = fetchingFiles.then(() => requireJson(path.join(target, 'package.json')))
    }

    const fetchedPkg = {
      fetchingPkg,
      fetchingFiles,
      id: resolution.id,
      fromCache: false,
      path: target,
      srcPath: resolution.type == 'directory'
        ? resolution.root
        : undefined,
      abort: async function () {
        try {
          await fetchingFiles
        } finally {
          return rimraf(target)
        }
      },
    }
    return fetchedPkg
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
      const data = await requireJson(path.join(fullpath, 'package.json'))
      return {
        fetchingPkg: Promise.resolve(data),
        fetchingFiles: Promise.resolve(false), // this property can be ignored by cached packages at all
        id: path.basename(fullpath),
        fromCache: true,
        path: fullpath,
        abort: () => Promise.resolve(),
      }
    }
  }
}

type FetchToStoreOptions = {
  fetchLocks: CachedPromises<Boolean>,
  target: string,
  resolution: Resolution,
  loggedPkg: LoggedPkg,
  got: Got,
  linkLocal: boolean,
  limitFetch: Function,
}

function fetchToStoreCached (opts: FetchToStoreOptions): Promise<Boolean> {
  return memoize(opts.fetchLocks, opts.resolution.id, async (): Promise<Boolean> => {
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
    await opts.limitFetch(() => fetchResolution(opts.resolution, targetStage, {
      got: opts.got,
      loggedPkg: opts.loggedPkg,
      linkLocal: opts.linkLocal,
    }))

    // fs.rename(oldPath, newPath) is an atomic operation, so we do it at the
    // end
    await fs.rename(targetStage, target)

    createShasum(target)

    return true
  })
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
