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

  return fs.stat(join(modules, pkgSpec.name))
    .catch((err) => {
      return resolve(pkgSpec)
        .then(set)
        .then(_ => log('resolved', pkgData))
        .then(_ => fs.stat(join(target, 'package.json'))) // todo: verify version?
        .catch((err) => {
          if (err.code !== 'ENOENT') throw err
          if (isLocked(target)) return Promise.resolve()
          return Promise.resolve()
          .then(_ => lock(target))
          .then(_ => log('downloading'))
          .then(_ => mkdirp(store))
          .then(_ => fetchIf(ctx.tmp, target, dist.tarball, dist.shasum, log))
          .then(_ => log('dependencies'))
          .then(_ => recurseDependencies())
          .then(_ => symlinkSelf())
          .then(_ => linkBins(modules, target))
          .then(_ => unlock(target))
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
      return symlink(join('..', '..', fullname), join(modules, name))
    }
  }

  // symlink itself to itself. this way, babel-runtime@5 can
  // require('babel-runtime') within itself.
  function symlinkSelf () {
    if (depth === 0) {
      return Promise.resolve()
    } else {
      return mkdirp(join(target, 'node_modules'))
        .then(_ => symlink(
          join('..', '..', fullname),
          join(target, 'node_modules', name)))
    }
  }

  function lock (path) {
    if (!ctx.lock) ctx.lock = {}
    ctx.lock[path] = true
  }

  function unlock (path) {
    if (ctx.lock) ctx.lock[path] = undefined
  }

  function isLocked (path) {
    return ctx.lock && ctx.lock[path]
  }

  function recurseDependencies () {
    // TODO: install to proper node_modules
    return installAll(ctx,
      pkgData.dependencies,
      join(target, 'node_modules'),
      { depth: depth + 1 })
  }
}

/*
 * Idempotent version of fetch()
 */

function fetchIf (tmpDir, target, tarball, shasum, log) {
  var uuid = getUuid()
  var tmp = join(tmpDir, uuid)

  return fs.stat(target)
    .then(_ => target)
    .catch(() => {
      return Promise.resolve()
        .then(_ => mkdirp(tmp))
        .then(_ => fetch(tmp, tarball, shasum, log))
        .then(_ => fs.rename(tmp, target))
        .then(_ => target)
        // TODO: this is wrong. it should rename *after* symlinking
        // and installing bins for better atomicity
    })
}

function linkBins (module, target) {
  // module == 'project/node_modules'
  // target == 'project/node_modules/.store/rimraf@2.5.1'

  var pkg = tryRequire(join(target, 'package.json'))
  if (!pkg || !pkg.bin) return

  Object.keys(pkg.bin).forEach((bin) => {
    // mkdirp(module, '.bin')
    // bin == 'rimraf'
    // pkg.bin[bin] == 'bin/rimraf'
    var actualBin = pkg.bin[bin]
    // chmod +x pkg.bin[bin]
    // symlink join('.bin', bin), join('..', '.store', 'rimraf@2.5.1', actualBin)
  })
}

function tryRequire (path) {
  try {
    return require(path)
  } catch (e) { }
}
