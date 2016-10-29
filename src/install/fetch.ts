import createDebug from '../debug'
const debug = createDebug('pnpm:install')
import npa = require('npm-package-arg')
import fs = require('mz/fs')
import {Stats} from 'fs'
import logger = require('@zkochan/logger')
import path = require('path')
import rimraf = require('rimraf-then')
import resolve, {ResolveResult} from '../resolve'
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import memoize, {CachedPromises} from '../memoize'
import {Package} from '../types'
import {Got} from '../network/got'
import {preserveSymlinks} from '../env'
import {InstallContext} from '../api/install'

export type FetchOptions = {
  keypath?: string[],
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  tag: string,
  got: Got,
}

export type FetchedPackage = {
  fetchingPkg: Promise<Package>,
  fetchingFiles: Promise<void>,
  path: string,
  srcPath?: string,
  id: string,
  name: string,
  fromCache: boolean,
  justFetched: boolean,
  abort(): Promise<void>,
}

export type InstallLog = (msg: string, data?: Object) => void

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
export default async function fetch (ctx: InstallContext, pkgRawSpec: string, modules: string, options: FetchOptions): Promise<FetchedPackage> {
  debug('installing ' + pkgRawSpec)

  // Preliminary spec data
  // => { raw, name, scope, type, spec, rawSpec }
  const spec = npa(pkgRawSpec)

  // Dependency path to the current package. Not actually needed anmyore
  // outside getting its length
  // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
  const keypath = (options && options.keypath || [])

  const log: InstallLog = logger.fork(spec).log.bind(null, 'progress', pkgRawSpec)

  try {
    // it might be a bundleDependency, in which case, don't bother
    const available = !options.force && await isAvailable(spec, modules)
    if (available) {
      const fetchedPkg = await saveCachedResolution()
      fetchedPkg.fetchingPkg.then(pkg => log('package.json', pkg))
      fetchedPkg.fetchingFiles.then(() => log('done'))
      return fetchedPkg
    }
    const res = await resolve(spec, {
      log,
      got: options.got,
      root: options.root,
      linkLocal: options.linkLocal,
      tag: options.tag
    })
    log('resolved', res)

    const target = path.join(options.storePath, res.id)
    const pkgPath = path.join(target, '_')

    const justFetched = !ctx.fetchLocks[res.id] &&
      (options.force || !(await exists(target)) || !ctx.store.packages[res.id])
    const fetchingFiles = !justFetched && !ctx.fetchLocks[res.id]
      ? Promise.resolve()
      : fetchToStoreCached({
        fetchLocks: ctx.fetchLocks,
        target,
        resolution: res,
        log,
        keypath,
        force: options.force,
      })

    const fetchingPkg = res.pkg
      ? Promise.resolve(res.pkg)
      : fetchingFiles.then(() => requireJson(path.join(pkgPath, 'package.json')))

    const fetchedPkg = {
      fetchingPkg,
      fetchingFiles,
      id: res.id,
      name: spec.name,
      fromCache: false,
      path: pkgPath,
      srcPath: res.root,
      justFetched,
      abort: () => rimraf(target),
    }
    fetchedPkg.fetchingPkg.then(pkg => log('package.json', pkg))
    fetchedPkg.fetchingFiles.then(() => log('done'))
    return fetchedPkg
  } catch (err) {
    log('error', err)
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
        name: spec.name,
        fromCache: true,
        path: fullpath,
        justFetched: false,
        abort: () => Promise.resolve(),
      }
    }
  }
}

type FetchToStoreOptions = {
  fetchLocks: CachedPromises<void>,
  target: string,
  resolution: ResolveResult,
  log: InstallLog,
  force: boolean,
  keypath: string[],
}

/**
 * Fetch to `.store/lodash@4.0.0`
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */
function fetchToStoreCached (opts: FetchToStoreOptions): Promise<void> {
  return memoize(opts.fetchLocks, opts.resolution.id, async function () {
    opts.log('download-queued')
    await opts.resolution.fetch(path.join(opts.target, '_'))

    const pkg = await requireJson(path.resolve(path.join(opts.target, '_', 'package.json')))

    opts.log('package.json', pkg)

    // this is not needed on Node.js >= 6.3.0
    if (!preserveSymlinks) {
      // symlink itself; . -> node_modules/lodash@4.0.0
      // this way it can require itself
      await symlinkSelf(opts.target, pkg, opts.keypath.length)
    }
  })
}

/**
 * Symlink a package into its own node_modules. this way, babel-runtime@5 can
 * require('babel-runtime') within itself.
 */
async function symlinkSelf (target: string, pkg: Package, depth: number) {
  debug(`symlinkSelf ${pkg.name}`)
  if (depth === 0) {
    return
  }
  await mkdirp(path.join(target, 'node_modules'))
  const src = isScoped(pkg.name)
    ? path.join('..', '..', '_')
    : path.join('..', '_')
  await linkDir(
    src,
    path.join(target, 'node_modules', pkg.name))
}

function isScoped (pkgName: string): boolean {
  return pkgName.indexOf('/') !== -1
}
