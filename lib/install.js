var Promise = require('./promise')
var debug = require('debug')('pnpm:install')
var npa = require('npm-package-arg')
var config = require('./config')
var join = require('path').join
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var resolve = require('./resolve')
var getUuid = require('node-uuid')
var symlink = require('./force_symlink')
var fs = require('mz/fs')
var basename = require('path').basename

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

module.exports = function install (ctx, pkg, modules, options) {
  var installAll = require('./install_multiple')
  debug('installing ' + pkg)

  var depth = (options && options.depth || 0)
  var pkgSpec = npa(pkg) // { raw, name, scope, type, spec, rawSpec }
  var pkgData  // { name, version, ... }
  var name     // 'lodash'
  var fullname // 'lodash@4.0.0'
  var dist     // { shasum, tarball }
  var target   // './node_modules/.store/lodash@4.0.0'
  var store = join(ctx.root, 'node_modules', '.store')
  var log = ctx.log(pkgSpec) // function
  var tmp = join(ctx.tmp, getUuid()) // 'node_modules/.tmp/000-11...'

  return fs.stat(join(modules, pkgSpec.name))
    .catch((err) => {
      return resolve(pkgSpec)
        .then(set)
        .then(_ => log('resolved', pkgData))
        .then(_ => fs.stat(join(target, 'package.json'))) // todo: verify version?
        .catch((err) => {
          if (err.code !== 'ENOENT') throw err
          if (isLocked(ctx, target)) return Promise.resolve()
          return Promise.resolve()
          .then(_ => lock(ctx, target))
          .then(_ => log('downloading'))
          .then(_ => mkdirp(store))
          .then(_ => mkdirp(tmp))
          // TODO: check for existence of target
          .then(_ => fetch(tmp, dist.tarball, dist.shasum, log))
          .then(_ => linkBins(modules, tmp, fullname))
          .then(_ => log('dependencies'))
          .then(_ => installAll(ctx,
            pkgData.dependencies,
            join(tmp, 'node_modules'),
            { depth: depth + 1 }))
          .then(_ => symlinkSelf(tmp, name, depth))
          .then(_ => fs.rename(tmp, target))
          .then(_ => unlock(ctx, target))
        })
        .then(_ => mkdirp(modules))
        .then(_ => doSymlink())
    })
    .then(_ => log('done'))
    .catch((err) => {
      log('error', err)
      throw err
    })

  // set metadata as fetched from resolve()
  function set (res) {
    pkgData = res
    fullname = '' + res.name + '@' + res.version
    target = join(store, fullname)
    name = res.name
    dist = res.dist
  }

  // perform the final symlinking of ./.store/x@1.0.0 => ./x.
  function doSymlink () {
    if (depth === 0) {
      return symlink(join('.store', fullname), join(modules, name))
    } else {
      // we'll go back to ..../.store here so the same module will act the same
      // on node_modules/.tmp
      return symlink(join('..', '..', '..', '.store', fullname), join(modules, name))
    }
  }
}

function lock (ctx, path) {
  if (!ctx.lock) ctx.lock = {}
  ctx.lock[path] = true
}

function unlock (ctx, path) {
  if (ctx.lock) ctx.lock[path] = undefined
}

function isLocked (ctx, path) {
  return ctx.lock && ctx.lock[path]
}

/*
 * symlink a package into its own node_modules. this way, babel-runtime@5 can
 * require('babel-runtime') within itself.
 */

function symlinkSelf (target, name, depth) {
  if (depth === 0) {
    return Promise.resolve()
  } else {
    return mkdirp(join(target, 'node_modules'))
      .then(_ => symlink(
        join('..'),
        join(target, 'node_modules', name)))
  }
}

/*
 * Links executables into `node_modules/.bin`
 *
 *     module = 'project/node_modules'
 *     target = 'project/node_modules/.tmp/a1b3c56...'
 *     linkBins(module, target, 'rimraf@2.5.1')
 *
 *     // node_modules/.bin/rimraf -> ../.store/rimraf@2.5.1/cmd.js
 */

function linkBins (module, target, fullname) {
  var pkg = tryRequire(join(target, 'package.json'))
  if (!pkg || !pkg.bin) return
  
  var bins = binify(pkg)

  return Object.keys(bins).map((bin) => {
    var actualBin = bins[bin]

    return Promise.resolve()
      .then(_ => fs.chmod(join(target, actualBin), 0o755))
      .then(_ => mkdirp(join(module, '.bin')))
      .then(_ => symlink(
        join('..', '.store', fullname, actualBin),
        join(module, '.bin', bin)))
  })
}

function binify (pkg) {
  if (typeof pkg.bin === 'string') {
    var obj = {}
    obj[pkg.name] = pkg.bin
    return obj
  }

  return pkg.bin
}

function tryRequire (path) {
  try {
    return require(path)
  } catch (e) { }
}
