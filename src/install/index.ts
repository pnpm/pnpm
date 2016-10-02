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
import relSymlink from '../fs/relSymlink'
import linkBundledDeps from './linkBundledDeps'
import isAvailable from './isAvailable'
import installAll from '../installMultiple'
import {InstallContext, CachedPromises} from '../api/install'
import {Package} from '../types'

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
  store: string
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
  id: string,
  keypath: string[],
  escapedName: string,
  fromCache: boolean
}

export type PackageContext = ResolveResult & {
  optional: boolean,
  linkLocal: boolean,
  keypath: string[],
  id: string,
  installationRoot: string,
  store: string,
  force: boolean
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
    const available = !options.force && await isAvailable(spec, modules)
    if (available) {
      installedPkg = await saveCachedResolution()
      log('package.json', installedPkg.pkg)
    } else {
      const res = await resolve(spec, {
        log,
        got: ctx.got,
        root: options.parentRoot || options.root,
        linkLocal: options.linkLocal
      })
      const freshPkg: PackageContext = saveResolution(res)
      log('resolved', freshPkg)
      await mkdirp(modules)
      const target = path.join(options.store, res.id)
      await buildToStoreCached(ctx, target, freshPkg, log)
      const pkg = requireJson(path.join(target, '_', 'package.json'))
      await symlinkToModules(path.join(target, '_'), modules)
      installedPkg = {
        pkg,
        optional,
        keypath,
        id: freshPkg.id,
        escapedName: spec.escapedName,
        fromCache: false
      }
      log('package.json', pkg)
    }

    if (!ctx.installs[installedPkg.id]) {
      ctx.installs[installedPkg.id] = installedPkg
    } else {
      ctx.installs[installedPkg.id].optional = ctx.installs[installedPkg.id].optional && installedPkg.optional
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
      id: res.id,
      root: res.root,
      fetch: res.fetch,
      installationRoot: options.root,
      store: options.store,
      force: options.force
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

    function save (fullpath: string): InstalledPackage {
      const data = requireJson(path.join(fullpath, 'package.json'))
      return {
        pkg: data,
        id: path.basename(fullpath),
        optional,
        keypath,
        escapedName: spec.escapedName,
        fromCache: true
      }
    }
  }
}

/**
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */
function buildToStoreCached (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog): Promise<Package> {
  // If a package is requested for a second time (usually when many packages depend
  // on the same thing), only resolve until it's fetched (not built).
  if (ctx.fetches[buildInfo.id]) return ctx.fetches[buildInfo.id]

  return make(target, () =>
    memoize(ctx.builds, buildInfo.id, async function () {
      await memoize(ctx.fetches, buildInfo.id, () => fetchToStore(ctx, target, buildInfo, log))
      return buildInStore(ctx, target, buildInfo, log)
    }),
    buildInfo.force
  )
}

/**
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * Fetches from npm, recurses to dependencies, runs lifecycle scripts, etc
 */
async function fetchToStore (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog) {
  // download and untar
  log('download-queued')
  return buildInfo.fetch(path.join(target, '_'), {log, got: ctx.got})

  // TODO: this is the point it becomes partially useable.
  // ie, it can now be symlinked into .store/foo@1.0.0.
  // it is only here that it should be available for ciruclar dependencies.
}

async function buildInStore (ctx: InstallContext, target: string, buildInfo: PackageContext, log: InstallLog) {
  const pkg = requireJson(path.resolve(path.join(target, '_', 'package.json')))
  log('package.json', pkg)

  await linkBundledDeps(path.join(target, '_'))

  // recurse down to dependencies
  log('dependencies')
  await installAll(ctx,
    pkg.dependencies || {},
    pkg.optionalDependencies || {},
    path.join(target, '_', 'node_modules'),
    {
      keypath: buildInfo.keypath.concat([ buildInfo.id ]),
      dependent: buildInfo.id,
      parentRoot: buildInfo.root,
      optional: buildInfo.optional,
      linkLocal: buildInfo.linkLocal,
      root: buildInfo.installationRoot,
      store: buildInfo.store,
      force: buildInfo.force
    })

  // symlink itself; . -> node_modules/lodash@4.0.0
  // this way it can require itself
  await symlinkSelf(target, pkg, buildInfo.keypath.length)

  ctx.piq = ctx.piq || []
  ctx.piq.push({
    path: target,
    pkgId: buildInfo.id
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
  await relSymlink(
    path.join('..', '_'),
    path.join(target, 'node_modules', escapeName(pkg.name)))
}

function escapeName (name: string) {
  return name && name.replace('/', '%2f')
}

/**
 * Perform the final symlinking of ./.store/x@1.0.0 -> ./x.
 *
 * @example
 *     target = '/node_modules/.store/lodash@4.0.0'
 *     modules = './node_modules'
 *     symlinkToModules(target, modules)
 */
async function symlinkToModules (target: string, modules: string) {
  // TODO: uncomment to make things fail
  const pkgData = requireJson(path.join(target, 'package.json'))
  if (!pkgData.name) { throw new Error('Invalid package.json for ' + target) }

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  const out = path.join(modules, pkgData.name)
  await mkdirp(path.dirname(out))
  await relSymlink(target, out)
}

/**
 * If `path` doesn't exist, run `fn()`.
 * If it exists (and installation is not forced), don't do anything.
 */
async function make (path: string, fn: Function, force: boolean) {
  try {
    await fs.stat(path)
    if (force) return fn()
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
    return fn()
  }
}

/**
 * Save promises for later
 */
function memoize (locks: CachedPromises, key: string, fn: () => Promise<void>) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
