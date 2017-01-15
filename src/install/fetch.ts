import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {ResolveResult, PackageSpec} from '../resolve'
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import memoize, {CachedPromises} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {InstallContext} from '../api/install'
import fetchRes from './fetchResolution'
import logStatus from '../logging/logInstallStatus'

export type FetchOptions = {
  keypath?: string[],
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  tag: string,
  got: Got,
  update?: boolean,
}

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<void>,
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

  // Dependency path to the current package. Not actually needed anmyore
  // outside getting its length
  // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
  const keypath = (options && options.keypath || [])

  try {
    let resolution = ctx.shrinkwrap[spec.raw]
    if (!resolution) {
      // it might be a bundleDependency, in which case, don't bother
      const available = !options.force && await isAvailable(spec, modules)
      if (available) {
        const fetchedPkg = await saveCachedResolution()
        return fetchedPkg
      }
    }
    if (!resolution || options.update) {
      resolution = await resolve(spec, {
        loggedPkg,
        got: options.got,
        root: options.root,
        linkLocal: options.linkLocal,
        tag: options.tag
      })
      if (resolution.tarball || resolution.repo) {
        ctx.shrinkwrap[spec.raw] = Object.assign({}, resolution)
        delete ctx.shrinkwrap[spec.raw].pkg
        delete ctx.shrinkwrap[spec.raw].fetch
        delete ctx.shrinkwrap[spec.raw].root
      }
    }

    const target = path.join(options.storePath, resolution.id)

    const fetchingFiles = fetchToStoreCached({
      fetchLocks: ctx.fetchLocks,
      target,
      resolution,
      loggedPkg,
      got: options.got,
      force: options.force,
    })

    const fetchingPkg = resolution.pkg
      ? Promise.resolve(resolution.pkg)
      : fetchingFiles.then(() => requireJson(path.join(target, 'package.json')))

    const fetchedPkg = {
      fetchingPkg,
      fetchingFiles,
      id: resolution.id,
      fromCache: false,
      path: target,
      srcPath: resolution.root,
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
  resolution: ResolveResult,
  loggedPkg: LoggedPkg,
  got: Got,
  force: boolean,
}

/**
 * Fetch to `.store/lodash@4.0.0`
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */
function fetchToStoreCached (opts: FetchToStoreOptions): Promise<void> {
  return memoize(opts.fetchLocks, opts.resolution.id, async function () {
    const target = opts.target
    const targetStage = `${opts.target}-stage`
    const targetExists = await exists(target)
    if (opts.force || !targetExists) {
      // We fetch into targetStage directory first and then fs.rename() it to the
      // target directory.

      await rimraf(targetStage)
      if (targetExists) {
        await rimraf(target)
      }

      logStatus({status: 'download-queued', pkg: opts.loggedPkg})
      await fetchRes(opts.resolution, targetStage, {got: opts.got, loggedPkg: opts.loggedPkg})

      // fs.rename(oldPath, newPath) is an atomic operation, so we do it at the
      // end
      await fs.rename(targetStage, target)
    }
    const pkg = await requireJson(path.join(target, 'package.json'))
  })
}
