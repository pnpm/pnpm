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
import {InstallContext, CachedPromises} from '../api/install'
import {Package} from '../types'
import symlinkToModules from './symlinkToModules'

export type PackageMeta = {
  rawSpec: string,
  optional: boolean
}

export type InstallationOptions = {
  optional?: boolean,
  keypath?: string[],
  parentRoot?: string,
  linkLocal: boolean,
  force: boolean,
  root: string,
  storePath: string,
  depth: number,
  tag: string
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
  dependencies: InstalledPackage[], // is needed to support flat tree
}

export type PackageContext = ResolveResult & {
  keypath: string[],
  id: string,
  force: boolean,
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
export default async function install (ctx: InstallContext, pkgMeta: PackageMeta, modules: string, options: InstallationOptions): Promise<InstalledPackage> {
  debug('installing ' + pkgMeta.rawSpec)
  if (!ctx.fetches) ctx.fetches = {}

  // Preliminary spec data
  // => { raw, name, scope, type, spec, rawSpec }
  const spec = npa(pkgMeta.rawSpec)

  const optional: boolean = pkgMeta.optional || options.optional === true

  // Dependency path to the current package. Not actually needed anmyore
  // outside getting its length
  // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
  const keypath = (options && options.keypath || [])

  const log: InstallLog = logger.fork(spec).log.bind(null, 'progress', pkgMeta.rawSpec)
  let installedPkg: InstalledPackage

  try {
    // it might be a bundleDependency, in which case, don't bother
    const available = !options.force && await isAvailable(spec, modules)
    if (available) {
      installedPkg = await saveCachedResolution()
      log('package.json', installedPkg.pkg)
    } else {
      const res = await resolve(spec, {
        log,
        got: ctx.got,
        root: options.parentRoot || options.root,
        linkLocal: options.linkLocal,
        tag: options.tag
      })
      const freshPkg: PackageContext = saveResolution(res)
      log('resolved', freshPkg)
      await mkdirp(modules)
      const target = path.join(options.storePath, res.id)
      await buildToStoreCached(ctx, target, freshPkg, log)
      const pkg = await requireJson(path.join(target, '_', 'package.json'))
      await symlinkToModules(path.join(target, '_'), modules)
      installedPkg = {
        pkg,
        optional,
        keypath,
        id: freshPkg.id,
        name: spec.name,
        fromCache: false,
        dependencies: [], // maybe nullable?
        path: path.join(target, '_'),
        srcPath: freshPkg.root,
        justFetched: !!ctx.fetches[freshPkg.id],
      }
      log('package.json', pkg)
    }

    log('done')
    return installedPkg
  } catch (err) {
    log('error', err)
    throw err
  }

  // set metadata as fetched from resolve()
  function saveResolution (res: ResolveResult): PackageContext {
    return {
      keypath,

      // Full name of package as it should be put in the store. Aim to make
      // this as friendly as possible as this will appear in stack traces.
      // => 'lodash@4.0.0'
      // => '@rstacruz!tap-spec@4.1.1'
      // => 'rstacruz!pnpm@0a1b382da'
      // => 'foobar@9a3b283ac'
      id: res.id,
      root: res.root,
      fetch: res.fetch,
      force: options.force,
    }
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
      }
    }
  }
}

/**
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */
async function buildToStoreCached (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog) {
  await memoize(ctx.fetches, buildInfo.id, async function () {
    if (!await exists(target) || buildInfo.force) {
      log('download-queued')
      await buildInfo.fetch(path.join(target, '_'), {log, got: ctx.got})
      
      const pkg = await requireJson(path.resolve(path.join(target, '_', 'package.json')))
      
      log('package.json', pkg)

      // symlink itself; . -> node_modules/lodash@4.0.0
      // this way it can require itself
      await symlinkSelf(target, pkg, buildInfo.keypath.length)
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

/**
 * Save promises for later
 */
function memoize <T>(locks: CachedPromises<T>, key: string, fn: () => Promise<T>) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
