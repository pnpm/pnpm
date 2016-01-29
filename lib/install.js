var Promise = require('./promise')
var debug = require('debug')('pnpm:install')
var npa = require('npm-package-arg')
var join = require('path').join
var dirname = require('path').dirname
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var resolve = require('./resolve')
var getUuid = require('node-uuid')
var symlink = require('./force_symlink')
var relSymlink = require('./rel_symlink')
var linkBins = require('./install/link_bins')
var fs = require('mz/fs')

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
  if (!ctx.fetches) ctx.fetches = {}

  var pkg = {
    // Preliminary spec data
    // => { raw, name, scope, type, spec, rawSpec }
    spec: npa(pkgSpec),

    // Dependency path to the current package
    // => ['babel-core@6.4.5', 'babylon@6.4.5', 'babel-runtime@5.8.35']
    keypath: (options && options.keypath || []),

    // Full name of package => 'lodash@4.0.0'
    fullname: undefined,

    // Distribution data from resolve() => { shasum, tarball }
    dist: undefined,

    // package.json data as retrieved from resolve() => { name, version, ... }
    data: undefined
  }

  var paths = {
    modules: modules, // './node_modules'

    // Final destination
    store: join(ctx.root, 'node_modules', '.store'),

    // Temporary destination while building
    tmp: join(ctx.root, 'node_modules', '.tmp', getUuid()),

    // Final destination => store + '/lodash@4.0.0'
    target: undefined
  }

  var log = ctx.log(pkg.spec) // function

  return fs.stat(join(modules, pkg.spec.name, 'package.json'))
    .catch(err => {
      if (err.code !== 'ENOENT') throw err
      return resolve(pkg.spec)
        .then(set)
        .then(_ => log('resolved', pkg.data))
        .then(_ => fs.stat(join(paths.target, 'package.json'))) // todo: verify version?
        .catch(err => {
          if (err.code !== 'ENOENT') throw err
          return isCircular(pkg)
            ? Promise.resolve()
            : memoize(ctx.fetches, pkg.fullname,
              _ => doFetch(ctx, paths, pkg, log))
        })
        .then(_ => mkdirp(paths.modules))
        .then(_ => symlinkToModules(paths.target, pkg.spec, paths.modules))
    })
    .then(_ => log('done'))
    .catch(err => {
      log('error', err)
      throw err
    })

  // set metadata as fetched from resolve()
  function set (res) {
    pkg.data = res
    pkg.fullname = '' + res.name.replace('/', '!') + '@' + res.version
    pkg.dist = res.dist
    paths.target = join(paths.store, pkg.fullname)
  }
}

// perform a fetch to `.store/lodash@4.0.0` (paths.target)
function doFetch (ctx, paths, pkg, log) {
  var installAll = require('./install_multiple')

  return Promise.resolve()
    .then(_ => mkdirp(dirname(paths.target)))
    .then(_ => symlink(paths.tmp, paths.target))
    .then(_ => log('downloading'))
    .then(_ => mkdirp(paths.store))
    .then(_ => mkdirp(paths.tmp))
    .then(_ => fetch(paths.tmp, pkg.dist.tarball, pkg.dist.shasum, log))
    .then(_ => linkBins(paths.modules, paths.tmp, paths.target))
    .then(_ => log('dependencies'))
    .then(_ => installAll(ctx,
      pkg.data.dependencies,
      join(paths.tmp, 'node_modules'),
      { keypath: pkg.keypath.concat([ pkg.fullname ]) }))
    .then(_ => symlinkSelf(paths.tmp, pkg.data, pkg.keypath.length))
    .then(_ => fs.unlink(paths.target))
    .then(_ => fs.rename(paths.tmp, paths.target))
}

function memoize (locks, key, fn) {
  if (locks && locks[key]) return locks[key]
  locks[key] = fn()
  return locks[key]
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
 *     name = 'lodash'
 *     modules = './node_modules'
 *     symlinkToModules(fullname, name, modules, 0)
 */

function symlinkToModules (target, pkg, modules) {
  // lodash -> .store/lodash@4.0.0
  // .store/foo@1.0.0/node_modules/lodash -> ../../../.store/lodash@4.0.0
  // .tmp/01234567890/node_modules/lodash -> ../../../.store/lodash@4.0.0
  if (pkg.scope) {
    debug('make scope dir', pkg.scope)
    return mkdirp(join(modules, pkg.scope))
      .then(relSymlink(target, join(modules, pkg.name)))
  }

  return relSymlink(target, join(modules, pkg.name))
}

/*
 * Checks if the current package is a circular dependency.
 */

function isCircular (pkg) {
  return pkg.keypath.indexOf(pkg.fullname) > -1
}
