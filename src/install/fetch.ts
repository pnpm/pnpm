import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {Resolution, PackageSpec} from '../resolve'
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import * as Shrinkwrap from '../fs/shrinkwrap'
import memoize, {CachedPromises} from '../memoize'
import {Package, FetchedPackage, LifecycleHooks} from '../types'
import {Got} from '../network/got'
import {InstallContext} from '../api/install'
import fetchResolution from './fetchResolution'
import logStatus from '../logging/logInstallStatus'

export type FetchOptions = {
  keypath?: string[],
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  tag: string,
  got: Got,
  lifecycle: LifecycleHooks,
  update?: boolean,
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

  // Dependency path to the current package. Not actually needed anmyore
  // outside getting its length
  // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
  const keypath = (options && options.keypath || [])

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
        lifecycle: ctx.lifecycle,
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
      lifecycle: options.lifecycle,
      got: options.got,
      linkLocal: options.linkLocal,
      force: options.force,
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
        fetchingFiles: Promise.resolve(),
        id: path.basename(fullpath),
        fromCache: true,
        path: fullpath,
        abort: () => Promise.resolve(),
      }
    }
  }
}

type FetchToStoreOptions = {
  fetchLocks: CachedPromises<void>,
  target: string,
  resolution: Resolution,
  loggedPkg: LoggedPkg,
  lifecycle: LifecycleHooks,
  got: Got,
  linkLocal: boolean,
  force: boolean,
}

/**
 * Fetch to `.store/lodash@4.0.0`
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */
function fetchToStoreCached (opts: FetchToStoreOptions): Promise<void> {
  return memoize(opts.fetchLocks, opts.resolution.id, async function () {
    const {packageWillFetch, packageDidFetch} = opts.lifecycle
    const target = opts.target
    const targetStage = `${opts.target}_stage`
    const targetExists = await exists(target)
    if (opts.force || !targetExists) {
      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      await rimraf(targetStage)
      if (targetExists) {
        await rimraf(target)
      }

      logStatus({status: 'download-queued', pkg: opts.loggedPkg})

      let fetched = false

      const fetchOptions = {
        got: opts.got,
        loggedPkg: opts.loggedPkg,
        linkLocal: opts.linkLocal,
      }

      if (packageWillFetch) {
        fetched = await packageWillFetch(targetStage, opts.resolution, fetchOptions)
      }

      if (!fetched) {
        await fetchResolution(opts.resolution, targetStage, fetchOptions)
      }

      if (packageDidFetch) {
        await packageDidFetch(targetStage, opts.resolution)
      }

      // fs.rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await fs.rename(targetStage, target)
    }
    const pkg = await requireJson(path.join(target, 'package.json'))
  })
}
