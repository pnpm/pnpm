import createDebug from '../debug'
const debug = createDebug('pnpm:install')
import npa = require('npm-package-arg')
import fs = require('mz/fs')
import {Stats} from 'fs'
import logger = require('@zkochan/logger')

import path = require('path')
const join = path.join
const dirname = path.dirname
const basename = path.basename
const abspath = path.resolve

import fetch from '../fetch'
import resolve, {ResolveResult, PackageDist} from '../resolve'

import mkdirp from '../fs/mkdirp'
import requireJson from '../fs/requireJson'
import relSymlink from '../fs/relSymlink'

import linkBundledDeps from './linkBundledDeps'
import isAvailable from './isAvailable'
import installAll from '../installMultiple'
import {InstallContext, CachedPromises} from '../api/install'
import {Package} from '../api/initCmd'

export type PackageMeta = {
  rawSpec: string,
  optional: boolean
}

export type InstallationOptions = {
  optional?: boolean,
  keypath?: string[],
  parentRoot?: string,
  linkLocal: boolean
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
  optional: boolean,
  fullname: string,
  keypath: string[],
  escapedName: string
}

export type PackageContext = {
  optional: boolean,
  linkLocal: boolean,
  keypath: string[],
  fullname: string,
  dist: PackageDist
}

export type InstallLog = (msg: string, data?: Object) => void

/*
 * Installs a package.
 *
 *     install(ctx, 'rimraf@2', './node_modules')
 *
 * Parameters:
 *
 * - `ctx` (Object) - the context.
 *   - `root` (String) - root path of the package.
 *   - `log` (Function) - logger
 *
 * What it does:
 *
 * - resolve() - resolve from registry.npmjs.org
 * - fetch() - download tarball into node_modules/.store/{name}@{version}
 * - recurse into its dependencies
 * - symlink node_modules/{name}
 * - symlink bins
 */

export default async function install (ctx: InstallContext, pkgMeta: PackageMeta, modules: string, options: InstallationOptions): Promise<InstalledPackage> {
  debug('installing ' + pkgMeta.rawSpec)
  if (!ctx.builds) ctx.builds = {}
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
    const available = await isAvailable(spec, modules)
    if (available) {
      installedPkg = await saveCachedResolution()
      log('package.json', installedPkg.pkg)
    } else {
      const res = await resolve(spec, {
        log,
        got: ctx.got,
        root: options.parentRoot || ctx.root,
        linkLocal: options.linkLocal
      })
      const freshPkg: PackageContext = saveResolution(res)
      log('resolved', freshPkg)
      await mkdirp(modules)
      let pkg: Package
      if (res.dist.location === 'dir' && options.linkLocal) {
        pkg = requireJson(join(res.dist.tarball, 'package.json'))
        await symlinkToModules(res.dist.tarball, modules)
      } else {
        const target = join(ctx.store, res.fullname)
        await buildToStoreCached(ctx, target, freshPkg, log)
        pkg = requireJson(join(target, '_', 'package.json'))
        await symlinkToModules(join(target, '_'), modules)
      }
      installedPkg = {
        pkg,
        optional,
        keypath,
        fullname: freshPkg.fullname,
        escapedName: spec.escapedName
      }
      log('package.json', pkg)
    }

    if (!ctx.installs[installedPkg.fullname]) {
      ctx.installs[installedPkg.fullname] = installedPkg
    } else {
      ctx.installs[installedPkg.fullname].optional = ctx.installs[installedPkg.fullname].optional && installedPkg.optional
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
      optional,
      keypath,
      linkLocal: options.linkLocal,

      // Full name of package as it should be put in the store. Aim to make
      // this as friendly as possible as this will appear in stack traces.
      // => 'lodash@4.0.0'
      // => '@rstacruz!tap-spec@4.1.1'
      // => 'rstacruz!pnpm@0a1b382da'
      // => 'foobar@9a3b283ac'
      fullname: res.fullname,

      // Distribution data from resolve() => { shasum, tarball }
      dist: res.dist
    }
  }

  async function saveCachedResolution (): Promise<InstalledPackage> {
    const target = join(modules, spec.name)
    const stat: Stats = await fs.lstat(target)
    if (stat.isSymbolicLink()) {
      const path = await fs.readlink(target)
      return save(abspath(path, target))
    }
    return save(target)

    function save (fullpath: string): InstalledPackage {
      const data = requireJson(join(fullpath, 'package.json'))
      return {
        pkg: data,
        fullname: basename(fullpath),
        optional,
        keypath,
        escapedName: spec.escapedName
      }
    }
  }
}

/*
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */

function buildToStoreCached (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog): Promise<Package> {
  // If a package is requested for a second time (usually when many packages depend
  // on the same thing), only resolve until it's fetched (not built).
  if (ctx.fetches[buildInfo.fullname]) return ctx.fetches[buildInfo.fullname]

  return make(target, () =>
    memoize(ctx.builds, buildInfo.fullname, async function () {
      await memoize(ctx.fetches, buildInfo.fullname, () => fetchToStore(ctx, target, buildInfo.dist, log))
      return buildInStore(ctx, target, buildInfo, log)
    })
  )
}

/*
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * Fetches from npm, recurses to dependencies, runs lifecycle scripts, etc
 */

async function fetchToStore (ctx: InstallContext, target: string, dist: PackageDist, log: InstallLog) {
  // download and untar
  log('download-queued')
  await mkdirp(join(target, '_'))
  await fetch(join(target, '_'), dist, {log, got: ctx.got})

  // if the dependency is a directory, the tarball was created temporarily
  // in order to mimic npm publish
  if (dist.location === 'dir') {
    await fs.unlink(dist.tarball)
  }

  // TODO: this is the point it becomes partially useable.
  // ie, it can now be symlinked into .store/foo@1.0.0.
  // it is only here that it should be available for ciruclar dependencies.
}

async function buildInStore (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog) {
  const pkg = requireJson(abspath(join(target, '_', 'package.json')))
  log('package.json', pkg)

  await linkBundledDeps(join(target, '_'))

  // recurse down to dependencies
  log('dependencies')
  await installAll(ctx,
    pkg.dependencies || {},
    pkg.optionalDependencies || {},
    join(target, '_', 'node_modules'),
    {
      keypath: buildInfo.keypath.concat([ buildInfo.fullname ]),
      dependent: buildInfo.fullname,
      parentRoot: buildInfo.dist.location !== 'remote'
        ? path.dirname(buildInfo.dist.tarball) : undefined,
      optional: buildInfo.optional,
      linkLocal: buildInfo.linkLocal
    })

  // symlink itself; . -> node_modules/lodash@4.0.0
  // this way it can require itself
  await symlinkSelf(target, pkg, buildInfo.keypath.length)

  ctx.piq = ctx.piq || []
  ctx.piq.push({
    path: target,
    pkgFullname: buildInfo.fullname
  })
}

/*
 * Symlink a package into its own node_modules. this way, babel-runtime@5 can
 * require('babel-runtime') within itself.
 */

async function symlinkSelf (target: string, pkg: Package, depth: number) {
  debug(`symlinkSelf ${pkg.name}`)
  if (depth === 0) {
    return
  }
  await mkdirp(join(target, 'node_modules'))
  await relSymlink(
    join('..', '_'),
    join(target, 'node_modules', escapeName(pkg.name)))
}

function escapeName (name: string) {
  return name && name.replace('/', '%2f')
}

/*
 * Perform the final symlinking of ./.store/x@1.0.0 -> ./x.
 *
 *     target = '/node_modules/.store/lodash@4.0.0'
 *     modules = './node_modules'
 *     symlinkToModules(fullname, modules)
 */

async function symlinkToModules (target: string, modules: string) {
  // TODO: uncomment to make things fail
  const pkgData = requireJson(join(target, 'package.json'))
  if (!pkgData.name) { throw new Error('Invalid package.json for ' + target) }

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  const out = join(modules, pkgData.name)
  await mkdirp(dirname(out))
  await relSymlink(target, out)
}

/*
 * If `path` doesn't exist, run `fn()`.
 * If it exists, don't do anything.
 */

async function make (path: string, fn: Function) {
  try {
    await fs.stat(path)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return fn()
  }
}

/*
 * Save promises for later
 */

function memoize (locks: CachedPromises, key: string, fn: () => Promise<void>) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
