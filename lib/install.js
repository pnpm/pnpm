var Promise = require('./promise')
var debug = require('debug')('pnpm:install')
var npa = require('npm-package-arg')
var join = require('path').join
var dirname = require('path').dirname
var abspath = require('path').resolve
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var resolve = require('./resolve')
var getUuid = require('node-uuid')
var symlink = require('./force_symlink')
var relSymlink = require('./rel_symlink')
var linkBins = require('./install/link_bins')
var linkBundledDeps = require('./install/link_bundled_deps')
var postInstall = require('./install/post_install')
var fs = require('mz/fs')
var obliterate = require('./fs/obliterate')
var requireJson = require('./fs/require_json')

/*
 * Installs a package.
 *
 *     install(ctx, 'rimraf@2', './node_modules')
 *
 * Parameters:
 *
 * - `ctx` (Object) - the context.
 *   - `root` (String) - root path of the package.
 *   - `tmp` (String) - temp dir
 *   - `log` (Function) - logger
 *
 * What it does:
 *
 * - resolve() - resolve from registry.npmjs.org
 * - fetch() - download tarball into node_modules/.tmp/{uuid}
 * - recurse into its dependencies
 * - run postinstall hooks
 * - move .tmp/{uuid} into node_modules/{name}@{version}
 * - symlink node_modules/{name}
 * - symlink bins
 */

module.exports = function install (ctx, pkgSpec, modules, options) {
  debug('installing ' + pkgSpec)
  if (!ctx.builds) ctx.builds = {}
  if (!ctx.fetches) ctx.fetches = {}

  var pkg = {
    // Preliminary spec data
    // => { raw, name, scope, type, spec, rawSpec }
    spec: npa(pkgSpec),

    // Dependency path to the current package
    // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
    keypath: (options && options.keypath || []),

    // Full name of package as it should be put in the store. Aim to make
    // this as friendly as possible as this will appear in stack traces.
    // => 'lodash@4.0.0'
    // => '@rstacruz!tap-spec@4.1.1'
    // => 'rstacruz!pnpm.js@0a1b382da'
    // => 'foobar@9a3b283ac'
    fullname: undefined,

    // Distribution data from resolve() => { shasum, tarball }
    dist: undefined,

    // package.json data as retrieved from resolve() => { name, version, ... }
    data: undefined
  }

  if (!pkg.spec.name) {
    throw new Error("Not supported: '" + pkgSpec + "'")
  }

  var paths = {
    // Module storage => './node_modules'
    modules: modules,

    // Temporary destination while building
    tmp: join(ctx.store, '..', '.tmp', getUuid()),

    // Final destination => store + '/lodash@4.0.0'
    target: undefined
  }

  var log = ctx.log(pkg.spec) // function

  return make(join(modules, pkg.spec.name), false, _ =>
      resolve(pkg.spec)
        .then(saveResolution)
        .then(_ => log('resolved', pkg.data))
        .then(_ => buildToStoreCached(ctx, paths, pkg, log))
        .then(_ => mkdirp(paths.modules))
        .then(_ => symlinkToModules(paths.target, paths.modules)))
    .then(_ => log('done'))
    .catch(err => {
      log('error', err)
      throw err
    })

  // set metadata as fetched from resolve()
  function saveResolution (res) {
    pkg.data = res
    pkg.fullname = '' + res.name.replace('/', '!') + '@' + res.version
    pkg.dist = res.dist
    paths.target = join(ctx.store, pkg.fullname)
  }
}

/*
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * If an ongoing build is already working, use it. Also, if that ongoing build
 * is part of the dependency chain (ie, it's a circular dependency), use its stub
 */

function buildToStoreCached (ctx, paths, pkg, log) {
  if (ctx.builds[pkg.fullname]) return ctx.fetches[pkg.fullname]

  if (isCircular(pkg)) {
    return Promise.resolve()
  } else {
    return make(paths.target, ctx.builds[pkg.fullname], _ =>
      memoize(ctx.builds, pkg.fullname, _ =>
        Promise.resolve()
          .then(_ => memoize(ctx.fetches, pkg.fullname, _ =>
            fetchToStore(ctx, paths, pkg, log)))
          .then(_ => buildInStore(ctx, paths, pkg, log))
      ))
  }
}

/*
 * Builds to `.store/lodash@4.0.0` (paths.target)
 * Fetches from npm, recurses to dependencies, runs lifecycle scripts, etc
 */

function fetchToStore (ctx, paths, pkg, log) {
  return Promise.resolve()
    // symlink .tmp/0a1b2c3d -> .store/lodash@4.0.0
    // so that when any other module requires it, it's available even
    // if it's partially built
    .then(_ => mkdirp(dirname(paths.target)))
    .then(_ => symlink(paths.tmp, paths.target))

    // download and untar
    .then(_ => log('downloading'))
    .then(_ => mkdirp(ctx.store))
    .then(_ => mkdirp(paths.tmp))
    .then(_ => fs.writeFile(join(paths.tmp, '.pnpm_inprogress'), '', 'utf-8'))
    .then(_ => fetch(paths.tmp, pkg.dist.tarball, pkg.dist.shasum, log))

    // TODO: this is the point it becomes partially useable.
    // ie, it can now be symlinked into .store/foo@1.0.0.
    // it is only here that it should be available for ciruclar dependencies.
}

function buildInStore (ctx, paths, pkg, log) {
  var installAll = require('./install_multiple')
  var fulldata

  return Promise.resolve()
    .then(_ => { fulldata = requireJson(abspath(join(paths.tmp, 'package.json'))) })

    // link node_modules/.bin
    .then(_ => linkBins(paths.modules, paths.tmp, paths.target))
    .then(_ => linkBundledDeps(paths.tmp))

    // recurse down to dependencies
    .then(_ => log('dependencies'))
    .then(_ => installAll(ctx,
      pkg.data.dependencies,
      join(paths.tmp, 'node_modules'),
      { keypath: pkg.keypath.concat([ pkg.fullname ]) }))

    // symlink itself; . -> node_modules/lodash@4.0.0
    // this way it can require itself
    .then(_ => symlinkSelf(paths.tmp, pkg.data, pkg.keypath.length))

    // postinstall hooks
    .then(_ => postInstall(paths.tmp, fulldata, installLogger(log, pkg)))

    // move to .store/lodash@4.0.0; remove the stub done earlier
    .then(_ => fs.unlink(join(paths.tmp, '.pnpm_inprogress')))
    .then(_ => fs.unlink(paths.target))
    .then(_ => fs.rename(paths.tmp, paths.target))
}

function installLogger (log, pkg) {
  return (stream, line) => {
    require('debug')('pnpm:post_install')('%s %s', pkg.fullname, line)
    log(stream, { name: pkg.fullname, line: line })
  }
}

/*
 * Symlink a package into its own node_modules. this way, babel-runtime@5 can
 * require('babel-runtime') within itself.
 */

function symlinkSelf (target, pkg, depth) {
  debug('symlinkSelf %s', pkg.name)
  if (depth === 0) {
    return Promise.resolve()
  } else {
    return mkdirp(join(target, 'node_modules'))
      .then(_ => symlink(
        join('..'),
        join(target, 'node_modules', pkg.name)))
  }
}

/*
 * Perform the final symlinking of ./.store/x@1.0.0 -> ./x.
 *
 *     target = '/node_modules/.store/lodash@4.0.0'
 *     modules = './node_modules'
 *     symlinkToModules(fullname, modules)
 */

function symlinkToModules (target, modules) {
  // TODO: uncomment to make things fail
  var pkgData = requireJson(join(target, 'package.json'))
  if (!pkgData.name) { throw new Error('Invalid package.json for ' + target) }

  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  // .tmp/01234567890/node_modules/lodash -> ../../../.store/lodash@4.0.0
  var out = join(modules, pkgData.name)
  return mkdirp(dirname(out))
    .then(_ => relSymlink(target, out))
}

/*
 * Checks if the current package is a circular dependency.
 */

function isCircular (pkg) {
  return pkg.keypath.indexOf(pkg.fullname) > -1
}

/*
 * If `path` doesn't exist, run `fn()`.
 * If it exists and is not in progress, don't do anything.
 * If it's in progress, check if we're working on it. If we're not,
 * obliterate it and run `fn()`.
 */

function make (path, isWorking, fn) {
  return fs.stat(path)
  .then(_ => {
    return fs.stat(join(path, '.pnpm_inprogress'))
    .then(_ => {
      if (!isWorking) return obliterate(path).then(fn)
    })
    .catch(err => { if (err.code !== 'ENOENT') throw err })
  })
  .catch(err => {
    if (err.code !== 'ENOENT') throw err
    return fn()
  })
}

/*
 * Save promises for later
 */

function memoize (locks, key, fn) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
}
