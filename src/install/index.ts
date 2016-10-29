import createDebug from '../debug'
const debug = createDebug('pnpm:install')
import npa = require('npm-package-arg')
import fs = require('mz/fs')
import {Stats} from 'fs'
import logger = require('@zkochan/logger')
import path = require('path')
import resolve, {ResolveResult} from '../resolve'
import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import linkDir from 'link-dir'
import exists = require('exists-file')
import isAvailable from './isAvailable'
import {CachedPromises} from '../api/install'
import {Package} from '../types'
import symlinkToModules from './symlinkToModules'
import {Got} from '../network/got'

export type InstallationOptions = {
  optional?: boolean,
  keypath?: string[],
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  depth: number,
  tag: string,
  got: Got,
}

export type PackageSpec = {
  raw: string,
  name: string,
  scope: string,
  type: string,
  spec: string,
  rawSpec: string
}

export type InstalledPackage = {
  pkg: Package,
  path: string,
  srcPath?: string,
  optional: boolean,
  id: string,
  keypath: string[],
  name: string,
  fromCache: boolean,
  justFetched: boolean, // TODO: maybe fromCache should be used
  firstFetch: boolean,
  dependencies: InstalledPackage[], // is needed to support flat tree
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
export default async function install (fetches: CachedPromises<void>, pkgRawSpec: string, modules: string, options: InstallationOptions): Promise<InstalledPackage> {
  debug('installing ' + pkgRawSpec)

  // Preliminary spec data
  // => { raw, name, scope, type, spec, rawSpec }
  const spec = npa(pkgRawSpec)

  const optional: boolean = options.optional === true

  // Dependency path to the current package. Not actually needed anmyore
  // outside getting its length
  // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
  const keypath = (options && options.keypath || [])

  const log: InstallLog = logger.fork(spec).log.bind(null, 'progress', pkgRawSpec)

  try {
    // it might be a bundleDependency, in which case, don't bother
    const available = !options.force && await isAvailable(spec, modules)
    if (available) {
      const installedPkg = await saveCachedResolution()
      log('package.json', installedPkg.pkg)
      log('done')
      return installedPkg
    }
    const res = await resolve(spec, {
      log,
      got: options.got,
      root: options.root,
      linkLocal: options.linkLocal,
      tag: options.tag
    })
    log('resolved', res)
    await mkdirp(modules)
    const target = path.join(options.storePath, res.id)

    let justFetched: boolean
    let firstFetch: boolean
    if (fetches[res.id]) {
      await fetches[res.id]
      justFetched = false
      firstFetch = false
    } else {
      firstFetch = true
      justFetched = await fetchToStoreCached({
        fetches,
        target,
        resolution: res,
        log,
        keypath,
        force: options.force,
      })
    }

    const pkg = await requireJson(path.join(target, '_', 'package.json'))
    await symlinkToModules(path.join(target, '_'), modules)
    const installedPkg = {
      pkg,
      optional,
      keypath,
      id: res.id,
      name: spec.name,
      fromCache: false,
      dependencies: [], // maybe nullable?
      path: path.join(target, '_'),
      srcPath: res.root,
      justFetched,
      firstFetch,
    }
    log('package.json', pkg)
    log('done')
    return installedPkg
  } catch (err) {
    log('error', err)
    throw err
  }

  async function saveCachedResolution (): Promise<InstalledPackage> {
    const target = path.join(modules, spec.name)
    const stat: Stats = await fs.lstat(target)
    if (stat.isSymbolicLink()) {
      const linkPath = await fs.readlink(target)
      return save(path.resolve(linkPath, target))
    }
    return save(target)

    async function save (fullpath: string): Promise<InstalledPackage> {
      const data = await requireJson(path.join(fullpath, 'package.json'))
      return {
        pkg: data,
        id: path.basename(fullpath),
        optional,
        keypath,
        name: spec.name,
        fromCache: true,
        dependencies: [],
        path: fullpath,
        justFetched: false,
        firstFetch: false,
      }
    }
  }
}

type FetchToStoreOptions = {
  fetches: CachedPromises<void>,
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
async function fetchToStoreCached (opts: FetchToStoreOptions): Promise<boolean> {
  return await memoize<boolean>(opts.fetches, opts.resolution.id, async function () {
    if (await exists(opts.target) && !opts.force) {
      return false
    }
    opts.log('download-queued')
    await opts.resolution.fetch(path.join(opts.target, '_'))

    const pkg = await requireJson(path.resolve(path.join(opts.target, '_', 'package.json')))

    opts.log('package.json', pkg)

    // symlink itself; . -> node_modules/lodash@4.0.0
    // this way it can require itself
    await symlinkSelf(opts.target, pkg, opts.keypath.length)
    return true
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

/**
 * Save promises for later
 */
function memoize <T>(locks: CachedPromises<T>, key: string, fn: () => Promise<T>) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
